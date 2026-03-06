//! Handlers for agent.* WS methods.

use anyhow::Result;
use chrono::Utc;
use hive_core::types::Agent;
use serde::Deserialize;
use serde_json::Value;

use crate::{communication as db_comm, db::DbPool};

#[derive(Deserialize)]
struct RegisterParams {
    id: String,
    name: String,
    #[serde(default)]
    tags: Vec<String>,
}

pub fn list(pool: &DbPool) -> Result<Value> {
    let agents = db_comm::list_agents(pool)?;
    Ok(serde_json::to_value(&agents)?)
}

pub fn register(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p: RegisterParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.id.trim().is_empty() {
        anyhow::bail!("id must not be empty");
    }

    let now = Utc::now();
    let agent = Agent {
        id: p.id,
        name: p.name,
        tags: p.tags,
        connected_at: Some(now),
        last_seen_at: Some(now),
    };
    db_comm::upsert_agent(pool, &agent)?;

    Ok(serde_json::json!({ "ok": true, "agent_id": agent.id }))
}
