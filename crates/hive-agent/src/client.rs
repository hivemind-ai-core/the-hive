//! WebSocket client connecting to hive-server with exponential backoff reconnection.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiMessage, MessageType};
use tokio::sync::{mpsc::{self, UnboundedReceiver, UnboundedSender}, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};
use uuid::Uuid;

/// Commands sent from the agent logic to the client task.
pub enum ClientCmd {
    Send(ApiMessage),
    Shutdown,
}

/// Shared map of in-flight requests. The client receive loop delivers responses
/// directly to the waiting caller's oneshot channel, preventing push messages
/// from being consumed and dropped while a request is pending.
pub type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<ApiMessage>>>>;

/// Start the WebSocket client.
///
/// Returns:
/// - `cmd_tx`: send outbound messages / shutdown signal
/// - `PendingRequests`: shared map for request/response correlation
/// - `push_rx`: receive server-initiated push messages
pub fn start(
    server_url: String,
    agent_id: String,
    agent_name: String,
    agent_tags: Vec<String>,
) -> (UnboundedSender<ClientCmd>, PendingRequests, UnboundedReceiver<ApiMessage>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<ClientCmd>();
    let (push_tx, push_rx) = mpsc::unbounded_channel::<ApiMessage>();
    let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(run_loop(
        server_url,
        agent_id,
        agent_name,
        agent_tags,
        cmd_rx,
        push_tx,
        Arc::clone(&pending),
    ));

    (cmd_tx, pending, push_rx)
}

async fn run_loop(
    server_url: String,
    agent_id: String,
    agent_name: String,
    agent_tags: Vec<String>,
    mut cmd_rx: UnboundedReceiver<ClientCmd>,
    push_tx: UnboundedSender<ApiMessage>,
    pending: PendingRequests,
) {
    let mut backoff = Duration::from_secs(1);
    const MAX_BACKOFF: Duration = Duration::from_secs(60);

    loop {
        let ws_url = format!("{server_url}?agent_id={agent_id}");

        match connect_async(&ws_url).await {
            Err(e) => {
                warn!("Connect failed ({ws_url}): {e}. Retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
            Ok((ws_stream, _)) => {
                info!("Connected to hive-server");
                backoff = Duration::from_secs(1);

                let (mut sink, mut stream) = ws_stream.split();

                // Register with the server immediately after connecting.
                let reg = request(
                    "agent.register",
                    Some(serde_json::json!({
                        "id": agent_id,
                        "name": agent_name,
                        "tags": agent_tags,
                    })),
                );
                if let Ok(json) = serde_json::to_string(&reg) {
                    if let Err(e) = sink.send(Message::text(json)).await {
                        warn!("Failed to send agent.register: {e}");
                    }
                }

                // Receive loop: route responses to pending oneshots, push messages to push_tx.
                let push_tx2 = push_tx.clone();
                let pending2 = Arc::clone(&pending);
                let recv_task = tokio::spawn(async move {
                    while let Some(frame) = stream.next().await {
                        match frame {
                            Ok(Message::Text(text)) => {
                                if let Ok(msg) = serde_json::from_str::<ApiMessage>(&text) {
                                    route_message(msg, &pending2, &push_tx2);
                                }
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            _ => {}
                        }
                    }
                });

                // Drive cmd_rx → WS sink.
                loop {
                    match cmd_rx.recv().await {
                        None | Some(ClientCmd::Shutdown) => {
                            recv_task.abort();
                            return;
                        }
                        Some(ClientCmd::Send(msg)) => {
                            match serde_json::to_string(&msg) {
                                Ok(json) => {
                                    if sink.send(Message::text(json)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => warn!("Serialize error: {e}"),
                            }
                        }
                    }

                    if recv_task.is_finished() {
                        break;
                    }
                }

                recv_task.abort();
                warn!("Disconnected. Retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Route an inbound message: responses go to waiting callers, pushes to push_tx.
fn route_message(
    msg: ApiMessage,
    pending: &PendingRequests,
    push_tx: &UnboundedSender<ApiMessage>,
) {
    // Route both Response and Error back to the pending oneshot so callers
    // don't time out waiting when the server returns an error.
    if matches!(msg.msg_type, MessageType::Response | MessageType::Error) {
        if let Ok(mut map) = pending.lock() {
            if let Some(tx) = map.remove(&msg.id) {
                let _ = tx.send(msg);
                return;
            }
        }
    }
    // Push message (or response with no pending waiter).
    let _ = push_tx.send(msg);
}

/// Build a request ApiMessage.
pub fn request(method: &str, params: Option<serde_json::Value>) -> ApiMessage {
    ApiMessage {
        msg_type: MessageType::Request,
        id: Uuid::new_v4().to_string(),
        method: Some(method.to_string()),
        params,
        result: None,
        error: None,
    }
}
