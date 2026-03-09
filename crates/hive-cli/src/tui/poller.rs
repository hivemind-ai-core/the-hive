//! Background WebSocket connection to hive-server.
//!
//! Connects as a special `__tui__` observer, seeds initial state with one-shot
//! requests, then stays connected and reacts to server-push events.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{Agent, ApiMessage, Comment, MessageType, Task, Topic};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::warn;

pub struct StateUpdate {
    pub agents: Vec<Agent>,
    pub tasks: Vec<Task>,
    pub topics: Vec<Topic>,
    pub topic_detail_id: Option<String>,
    pub topic_comments: Vec<Comment>,
}

/// Commands sent from the TUI to the poller to perform server actions.
pub enum TuiCmd {
    SendPush { to_agent_id: String, content: String },
    CreateTopic { title: String, content: String },
    CreateTask { title: String, description: String, tags: Vec<String> },
    CreateComment { topic_id: String, content: String },
    UpdateTask { id: String, title: String, description: String, tags: Vec<String> },
    SetTaskStatus { id: String, status: String },
    FetchTopic { topic_id: String },
}

/// Spawn a background thread that maintains a WS connection to `server_url`
/// and sends `StateUpdate` values through `tx` whenever state changes.
/// Returns a `Sender<TuiCmd>` for sending commands to the server.
pub fn spawn(server_url: String, tx: std::sync::mpsc::Sender<StateUpdate>) -> UnboundedSender<TuiCmd> {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TuiCmd>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("poller tokio runtime");
        rt.block_on(run(server_url, tx, cmd_rx));
    });
    cmd_tx
}

async fn run(server_url: String, tx: std::sync::mpsc::Sender<StateUpdate>, mut cmd_rx: UnboundedReceiver<TuiCmd>) {
    loop {
        if let Err(e) = connect_and_listen(&server_url, &tx, &mut cmd_rx).await {
            warn!("TUI server connection lost: {e}");
        }
        // Retry after a short delay if the TUI is still alive.
        if tx.send(StateUpdate { agents: vec![], tasks: vec![], topics: vec![], topic_detail_id: None, topic_comments: vec![] }).is_err() {
            break; // TUI has exited
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_and_listen(
    server_url: &str,
    tx: &std::sync::mpsc::Sender<StateUpdate>,
    cmd_rx: &mut UnboundedReceiver<TuiCmd>,
) -> anyhow::Result<()> {
    let url = format!("{server_url}?agent_id=__tui__");
    let (mut ws, _) = connect_async(&url).await?;

    let mut agents: Vec<Agent> = vec![];
    let mut tasks: Vec<Task> = vec![];
    let mut topics: Vec<Topic> = vec![];
    let mut topic_detail_id: Option<String> = None;
    let mut topic_comments: Vec<Comment> = vec![];

    // Seed initial state.
    send_request(&mut ws, "seed-agents", "agent.list", None).await?;
    send_request(&mut ws, "seed-tasks", "task.list", None).await?;
    send_request(&mut ws, "seed-topics", "topic.list", None).await?;

    loop {
        tokio::select! {
            raw = ws.next() => {
                let text = match raw {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                };
                let msg: ApiMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let state_changed = match msg.msg_type {
                    MessageType::Response => {
                        let Some(result) = msg.result else { continue };
                        match msg.id.as_str() {
                            "seed-agents" => {
                                agents = serde_json::from_value(result).unwrap_or_default();
                                true
                            }
                            "seed-tasks" => {
                                tasks = serde_json::from_value(result).unwrap_or_default();
                                true
                            }
                            "seed-topics" => {
                                topics = serde_json::from_value(result).unwrap_or_default();
                                true
                            }
                            "fetch-topic" => {
                                if let Some(id) = result.get("topic").and_then(|t| t.get("id")).and_then(|v| v.as_str()) {
                                    topic_detail_id = Some(id.to_string());
                                }
                                topic_comments = result.get("comments")
                                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                                    .unwrap_or_default();
                                true
                            }
                            _ => false,
                        }
                    }
                    MessageType::Push => {
                        let method = msg.method.as_deref().unwrap_or("");
                        let Some(params) = msg.params else { continue };
                        match method {
                            "agents.updated" => {
                                agents = serde_json::from_value(params).unwrap_or_default();
                                true
                            }
                            "tasks.updated" => {
                                tasks = serde_json::from_value(params).unwrap_or_default();
                                true
                            }
                            "topics.updated" => {
                                topics = serde_json::from_value(params).unwrap_or_default();
                                true
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                };

                if state_changed {
                    if tx.send(StateUpdate {
                        agents: agents.clone(),
                        tasks: tasks.clone(),
                        topics: topics.clone(),
                        topic_detail_id: topic_detail_id.clone(),
                        topic_comments: topic_comments.clone(),
                    }).is_err() {
                        return Ok(()); // TUI has exited
                    }
                }
            }

            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { return Ok(()); };
                match cmd {
                    TuiCmd::SendPush { to_agent_id, content } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "push.send",
                            Some(serde_json::json!({ "to_agent_id": to_agent_id, "content": content })),
                        ).await;
                    }
                    TuiCmd::CreateTopic { title, content } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "topic.create",
                            Some(serde_json::json!({ "title": title, "content": content, "creator_agent_id": "__tui__" })),
                        ).await;
                    }
                    TuiCmd::CreateTask { title, description, tags } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "task.create",
                            Some(serde_json::json!({ "title": title, "description": description, "tags": tags })),
                        ).await;
                    }
                    TuiCmd::SetTaskStatus { id, status } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "task.update",
                            Some(serde_json::json!({ "id": id, "status": status })),
                        ).await;
                    }
                    TuiCmd::UpdateTask { id, title, description, tags } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "task.update",
                            Some(serde_json::json!({ "id": id, "title": title, "description": description, "tags": tags })),
                        ).await;
                    }
                    TuiCmd::CreateComment { topic_id, content } => {
                        let _ = send_request(
                            &mut ws,
                            &uuid::Uuid::new_v4().to_string(),
                            "topic.comment",
                            Some(serde_json::json!({ "topic_id": topic_id, "content": content, "creator_agent_id": "__tui__" })),
                        ).await;
                    }
                    TuiCmd::FetchTopic { topic_id } => {
                        let _ = send_request(
                            &mut ws,
                            "fetch-topic",
                            "topic.get",
                            Some(serde_json::json!({ "id": topic_id })),
                        ).await;
                    }
                }
            }
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
