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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serde_json::json;

    fn open_test_db() -> crate::db::DbPool {
        let pool = db::open(":memory:").unwrap();
        db::run_migrations(&pool).unwrap();
        pool
    }

    // ── send handler ────────────────────────────────────────────────────────

    #[test]
    fn send_stores_message() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        let result = send(&pool, &registry, "agent-a", Some(json!({
            "to_agent_id": "agent-b",
            "content": "Hello!"
        }))).unwrap();
        assert!(result["id"].is_string());
    }

    #[test]
    fn send_missing_to_agent_id_errors() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(send(&pool, &registry, "a", Some(json!({"content": "hi"}))).is_err());
    }

    #[test]
    fn send_missing_content_errors() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(send(&pool, &registry, "a", Some(json!({"to_agent_id": "b"}))).is_err());
    }

    #[test]
    fn send_empty_to_agent_id_errors() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(send(&pool, &registry, "a", Some(json!({"to_agent_id": "  ", "content": "hi"}))).is_err());
    }

    #[test]
    fn send_null_params_errors() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(send(&pool, &registry, "a", None).is_err());
    }

    // ── list handler ────────────────────────────────────────────────────────

    #[test]
    fn list_empty_when_no_messages() {
        let pool = open_test_db();
        let result = list(&pool, "agent-a").unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn list_returns_pending_messages() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        send(&pool, &registry, "a", Some(json!({"to_agent_id": "b", "content": "hi"}))).unwrap();
        let result = list(&pool, "b").unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["content"], "hi");
    }

    #[test]
    fn list_scoped_to_recipient() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        send(&pool, &registry, "a", Some(json!({"to_agent_id": "b", "content": "for b"}))).unwrap();
        let result = list(&pool, "c").unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }

    // ── ack handler ─────────────────────────────────────────────────────────

    #[test]
    fn ack_marks_messages_delivered() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        let msg = send(&pool, &registry, "a", Some(json!({"to_agent_id": "b", "content": "hi"}))).unwrap();
        let msg_id = msg["id"].as_str().unwrap();

        let result = ack(&pool, Some(json!({"message_ids": [msg_id]}))).unwrap();
        assert_eq!(result["acked"], 1);

        // Now list is empty
        let result = list(&pool, "b").unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn ack_empty_array_is_ok() {
        let pool = open_test_db();
        let result = ack(&pool, Some(json!({"message_ids": []}))).unwrap();
        assert_eq!(result["acked"], 0);
    }

    #[test]
    fn ack_missing_message_ids_errors() {
        let pool = open_test_db();
        assert!(ack(&pool, Some(json!({}))).is_err());
    }

    #[test]
    fn ack_null_params_errors() {
        let pool = open_test_db();
        assert!(ack(&pool, None).is_err());
    }
}
