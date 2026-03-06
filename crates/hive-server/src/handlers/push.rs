//! Handlers for push.* WS methods.

use anyhow::Result;
use hive_core::types::PushMessage;
use serde::Deserialize;
use serde_json::Value;
use axum::extract::ws::Message;
use tracing::warn;

use crate::{communication as db_comm, db::DbPool, state::Clients};

#[derive(Deserialize)]
struct SendParams {
    to_agent_id: String,
    content: String,
}

/// Store a push message and deliver it immediately if the target is connected.
pub fn send(
    pool: &DbPool,
    clients: &Clients,
    from_agent_id: &str,
    params: Option<Value>,
) -> Result<Value> {
    let p: SendParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.to_agent_id.trim().is_empty() {
        anyhow::bail!("to_agent_id must not be empty");
    }

    let msg = PushMessage::new(
        p.to_agent_id.clone(),
        p.content,
        Some(from_agent_id.to_string()),
    );
    db_comm::insert_message(pool, &msg)?;

    // Attempt live delivery if the target is connected right now.
    if let Ok(guard) = clients.lock() {
        if let Some(tx) = guard.get(&p.to_agent_id) {
            let push = crate::ws::make_push(serde_json::to_value(&msg)?);
            if let Ok(json) = serde_json::to_string(&push) {
                let _ = tx.send(Message::Text(json.into()));
                drop(guard);
                if let Err(e) = db_comm::mark_delivered(pool, &msg.id) {
                    warn!("Failed to mark message {} as delivered: {e}", msg.id);
                }
            }
        }
    }

    Ok(serde_json::json!({ "id": msg.id }))
}

/// Return undelivered messages for the calling agent.
pub fn list(pool: &DbPool, agent_id: &str) -> Result<Value> {
    let msgs = db_comm::pending_messages(pool, agent_id)?;
    Ok(serde_json::to_value(&msgs)?)
}

/// Mark a batch of messages as delivered (explicit ACK from agent).
pub fn ack(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let ids: Vec<String> = params
        .as_ref()
        .and_then(|v| v.get("message_ids"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("params.message_ids (array) is required"))?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    for id in &ids {
        db_comm::mark_delivered(pool, id)?;
    }
    Ok(serde_json::json!({ "ok": true, "acked": ids.len() }))
}
