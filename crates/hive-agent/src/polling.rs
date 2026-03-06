//! Task polling loop: requests the next available task and drives execution.

use std::time::Duration;

use hive_core::types::{ApiMessage, MessageType, PushMessage, Task};
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use tracing::{info, warn};
use uuid::Uuid;

use crate::client::{ClientCmd, PendingRequests};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Run the task polling loop.
///
/// Sends `task.get_next` requests and drives task execution via `on_task`.
/// Responses are delivered directly via oneshot channels (see `send_request`),
/// so push messages are never accidentally consumed.
pub async fn run(
    agent_id: String,
    agent_tags: Vec<String>,
    coding_agent: String,
    cmd_tx: UnboundedSender<ClientCmd>,
    pending: PendingRequests,
    mut on_task: impl FnMut(Task) + Send,
) {
    let mut backoff = POLL_INTERVAL;
    let tag = agent_tags.first().cloned();

    loop {
        let msg = ApiMessage {
            msg_type: MessageType::Request,
            id: Uuid::new_v4().to_string(),
            method: Some("task.get_next".to_string()),
            params: Some(serde_json::json!({
                "agent_id": agent_id,
                "tag": tag,
            })),
            result: None,
            error: None,
        };

        match send_request(&cmd_tx, &pending, msg).await {
            None => {
                warn!("No response (channel closed or timeout), retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
            Some(msg) if msg.result.as_ref().map(|v| v.is_null()).unwrap_or(false) => {
                // No task available — check for unread push messages and execute if present.
                tracing::debug!("No task available, polling again in {POLL_INTERVAL:?}");
                handle_idle_push_messages(&agent_id, &coding_agent, &cmd_tx, &pending).await;
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Some(msg) if msg.error.is_some() => {
                let err = msg.error.as_ref().unwrap();
                warn!("task.get_next error {}: {}", err.code, err.message);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
            Some(msg) => {
                if let Some(result) = msg.result {
                    match serde_json::from_value::<Task>(result) {
                        Ok(task) => {
                            info!("Claimed task: {} ({})", task.title, task.id);
                            backoff = POLL_INTERVAL;
                            on_task(task);
                        }
                        Err(e) => warn!("Failed to deserialize task: {e}"),
                    }
                }
            }
        }
    }
}

/// When idle (no task), fetch unread push messages. If any, run the coding agent
/// to handle them, then acknowledge them. If none, do nothing.
async fn handle_idle_push_messages(
    agent_id: &str,
    coding_agent: &str,
    cmd_tx: &UnboundedSender<ClientCmd>,
    pending: &PendingRequests,
) {
    let req = ApiMessage {
        msg_type: MessageType::Request,
        id: Uuid::new_v4().to_string(),
        method: Some("push.list".to_string()),
        params: Some(serde_json::json!({})),
        result: None,
        error: None,
    };

    let messages: Vec<PushMessage> = match send_request(cmd_tx, pending, req).await {
        Some(resp) => resp.result
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        None => return,
    };

    if messages.is_empty() {
        return;
    }

    for m in &messages {
        let from = m.from_agent_id.as_deref().unwrap_or("unknown");
        info!("Unread push from {from}: {}", m.content);
    }

    let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();

    match crate::executor::run_push_only(coding_agent, agent_id, &messages).await {
        Ok(r) => info!("Push-only execution finished (exit {})", r.exit_code),
        Err(e) => warn!("Push-only execution failed: {e}"),
    }

    // Acknowledge all messages regardless of execution result.
    let ack_req = crate::client::request(
        "push.ack",
        Some(serde_json::json!({ "message_ids": message_ids })),
    );
    if let Err(e) = cmd_tx.send(crate::client::ClientCmd::Send(ack_req)) {
        warn!("Failed to send push.ack: {e}");
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
