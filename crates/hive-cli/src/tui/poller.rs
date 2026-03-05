//! Background WebSocket connection to hive-server.
//!
//! Connects as a special `__tui__` observer, seeds initial state with one-shot
//! requests, then stays connected and reacts to server-push events.

use std::sync::mpsc::Sender;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{Agent, ApiMessage, MessageType, Task};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::warn;

pub struct StateUpdate {
    pub agents: Vec<Agent>,
    pub tasks: Vec<Task>,
}

/// Spawn a background thread that maintains a WS connection to `server_url`
/// and sends `StateUpdate` values through `tx` whenever state changes.
pub fn spawn(server_url: String, tx: Sender<StateUpdate>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("poller tokio runtime");
        rt.block_on(run(server_url, tx));
    });
}

async fn run(server_url: String, tx: Sender<StateUpdate>) {
    loop {
        if let Err(e) = connect_and_listen(&server_url, &tx).await {
            warn!("TUI server connection lost: {e}");
        }
        // Retry after a short delay if the TUI is still alive.
        if tx.send(StateUpdate { agents: vec![], tasks: vec![] }).is_err() {
            break; // TUI has exited
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_and_listen(server_url: &str, tx: &Sender<StateUpdate>) -> anyhow::Result<()> {
    let url = format!("{server_url}?agent_id=__tui__");
    let (mut ws, _) = connect_async(&url).await?;

    let mut agents: Vec<Agent> = vec![];
    let mut tasks: Vec<Task> = vec![];

    // Seed initial state.
    send_request(&mut ws, "seed-agents", "agent.list", None).await?;
    send_request(&mut ws, "seed-tasks", "task.list", None).await?;

    while let Some(raw) = ws.next().await {
        let text = match raw? {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let msg: ApiMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match msg.msg_type {
            MessageType::Response => {
                let Some(result) = msg.result else { continue };
                // Both seed responses are arrays of agents or tasks; distinguish by shape.
                match msg.id.as_str() {
                    "seed-agents" => {
                        agents = serde_json::from_value(result).unwrap_or_default();
                    }
                    "seed-tasks" => {
                        tasks = serde_json::from_value(result).unwrap_or_default();
                    }
                    _ => continue,
                }
            }
            MessageType::Push => {
                let method = msg.method.as_deref().unwrap_or("");
                let Some(params) = msg.params else { continue };
                match method {
                    "agents.updated" => {
                        agents = serde_json::from_value(params).unwrap_or_default();
                    }
                    "tasks.updated" => {
                        tasks = serde_json::from_value(params).unwrap_or_default();
                    }
                    _ => continue,
                }
            }
            _ => continue,
        }

        if tx.send(StateUpdate { agents: agents.clone(), tasks: tasks.clone() }).is_err() {
            break; // TUI has exited
        }
    }

    Ok(())
}

async fn send_request(
    ws: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    id: &str,
    method: &str,
    params: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let msg = ApiMessage {
        msg_type: MessageType::Request,
        id: id.to_string(),
        method: Some(method.to_string()),
        params,
        result: None,
        error: None,
    };
    let json = serde_json::to_string(&msg)?;
    ws.send(Message::text(json)).await?;
    Ok(())
}
