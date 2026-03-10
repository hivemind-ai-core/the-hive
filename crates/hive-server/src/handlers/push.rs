//! Handlers for push.* WS methods.

use anyhow::Result;
use hive_core::types::PushMessage;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent_registry::{self, AgentRegistry},
    communication as db_comm,
    db::DbPool,
};

#[derive(Deserialize)]
struct SendParams {
    to_agent_id: String,
    content: String,
}

/// Store a push message and attempt live delivery via push.notify.
///
/// Does NOT mark as delivered — only push.ack is authoritative.
/// This ensures the message is always available via push.list until
/// explicitly acknowledged, even if the agent was busy when it arrived.
pub fn send(
    pool: &DbPool,
    registry: &AgentRegistry,
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

    // Attempt live delivery via push.notify. The agent will ack it explicitly.
    if let Ok(messages_val) = serde_json::to_value(std::slice::from_ref(&msg)) {
        agent_registry::notify_agent(registry, &p.to_agent_id, &messages_val);
    }

    Ok(serde_json::json!({ "id": msg.id }))
}

/// Return undelivered messages for the calling agent.
pub fn list(pool: &DbPool, agent_id: &str) -> Result<Value> {
    let msgs = db_comm::pending_messages(pool, agent_id)?;
    Ok(serde_json::to_value(&msgs)?)
}

/// Mark a batch of messages as delivered (explicit ACK from agent).
#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
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
