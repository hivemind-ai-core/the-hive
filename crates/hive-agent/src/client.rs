//! WebSocket client connecting to hive-server with exponential backoff reconnection.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiMessage, MessageType};
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};
use uuid::Uuid;

/// How long to wait for a response to a request before giving up.
/// 30 seconds is sufficient for all current server operations.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Commands sent from the agent logic to the client task.
#[derive(Debug)]
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
) -> (
    UnboundedSender<ClientCmd>,
    PendingRequests,
    UnboundedReceiver<ApiMessage>,
) {
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
                        "capacity_max": 1,
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
                        Some(ClientCmd::Send(msg)) => match serde_json::to_string(&msg) {
                            Ok(json) => {
                                if sink.send(Message::text(json)).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => warn!("Serialize error: {e}"),
                        },
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

/// Route an inbound message: responses go to waiting callers, pushes to `push_tx`.
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

/// Build a request `ApiMessage`.
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

/// Register a pending oneshot, send the request, and await the response with a
/// 30-second timeout. Returns `None` on channel close or timeout.
pub async fn send_request(
    cmd_tx: &UnboundedSender<ClientCmd>,
    pending: &PendingRequests,
    msg: ApiMessage,
) -> Option<ApiMessage> {
    let (tx, rx) = oneshot::channel();

    // Register before sending to avoid a race where the response arrives first.
    if let Ok(mut map) = pending.lock() {
        map.insert(msg.id.clone(), tx);
    } else {
        return None;
    }

    if cmd_tx.send(ClientCmd::Send(msg)).is_err() {
        return None;
    }

    match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
        Ok(Ok(response)) => Some(response),
        Ok(Err(_)) => None, // sender dropped
        Err(_) => {
            warn!("Request timed out after {REQUEST_TIMEOUT:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── request ─────────────────────────────────────────────────────────────

    #[test]
    fn request_builds_correct_message() {
        let msg = request("task.create", Some(serde_json::json!({"title": "Test"})));
        assert_eq!(msg.msg_type, MessageType::Request);
        assert_eq!(msg.method.as_deref(), Some("task.create"));
        assert_eq!(msg.params.as_ref().unwrap()["title"], "Test");
        assert!(msg.result.is_none());
        assert!(msg.error.is_none());
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn request_with_no_params() {
        let msg = request("push.list", None);
        assert_eq!(msg.method.as_deref(), Some("push.list"));
        assert!(msg.params.is_none());
    }

    #[test]
    fn request_generates_unique_ids() {
        let msg1 = request("a", None);
        let msg2 = request("b", None);
        assert_ne!(msg1.id, msg2.id);
    }

    // ── route_message ───────────────────────────────────────────────────────

    #[test]
    fn route_response_to_pending_caller() {
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let (push_tx, mut push_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = oneshot::channel();

        pending.lock().unwrap().insert("req-1".to_string(), tx);

        let response = ApiMessage {
            msg_type: MessageType::Response,
            id: "req-1".to_string(),
            method: None,
            params: None,
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        };

        route_message(response, &pending, &push_tx);

        // Caller should receive the response
        let received = rx.try_recv().unwrap();
        assert_eq!(received.result.unwrap()["ok"], true);

        // Push channel should be empty
        assert!(push_rx.try_recv().is_err());

        // Pending map should be cleared
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn route_error_to_pending_caller() {
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let (push_tx, _push_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = oneshot::channel();

        pending.lock().unwrap().insert("req-1".to_string(), tx);

        let error = ApiMessage {
            msg_type: MessageType::Error,
            id: "req-1".to_string(),
            method: None,
            params: None,
            result: None,
            error: Some(hive_core::types::ApiError {
                code: 500,
                message: "boom".to_string(),
            }),
        };

        route_message(error, &pending, &push_tx);

        let received = rx.try_recv().unwrap();
        assert_eq!(received.error.unwrap().code, 500);
    }

    #[test]
    fn route_push_message_to_push_channel() {
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let (push_tx, mut push_rx) = mpsc::unbounded_channel();

        let push = ApiMessage {
            msg_type: MessageType::Push,
            id: "push-1".to_string(),
            method: Some("task.assign".to_string()),
            params: Some(serde_json::json!({"task": {}})),
            result: None,
            error: None,
        };

        route_message(push, &pending, &push_tx);

        let received = push_rx.try_recv().unwrap();
        assert_eq!(received.method.as_deref(), Some("task.assign"));
    }

    #[test]
    fn route_response_without_pending_goes_to_push() {
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let (push_tx, mut push_rx) = mpsc::unbounded_channel();

        let orphan = ApiMessage {
            msg_type: MessageType::Response,
            id: "orphan".to_string(),
            method: None,
            params: None,
            result: Some(serde_json::json!(42)),
            error: None,
        };

        route_message(orphan, &pending, &push_tx);

        // Falls through to push channel
        assert!(push_rx.try_recv().is_ok());
    }
}
