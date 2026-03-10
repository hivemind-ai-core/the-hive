//! Handlers for agent.* WS methods.

use anyhow::Result;
use chrono::Utc;
use hive_core::types::Agent;
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::{
    agent_registry::AgentRegistry,
    communication as db_comm,
    db::DbPool,
    tasks as db_tasks,
};

#[derive(Deserialize)]
struct RegisterParams {
    id: String,
    name: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_capacity_max")]
    capacity_max: u8,
}

fn default_capacity_max() -> u8 { 1 }

#[derive(Deserialize)]
struct StatusParams {
    active_tasks: u8,
}

pub fn list(pool: &DbPool) -> Result<Value> {
    let agents = db_comm::list_agents(pool)?;
    Ok(serde_json::to_value(&agents)?)
}

pub fn register(pool: &DbPool, registry: &AgentRegistry, params: Option<Value>) -> Result<Value> {
    let p: RegisterParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.id.trim().is_empty() {
        anyhow::bail!("id must not be empty");
    }

    let now = Utc::now();
    let agent = Agent {
        id: p.id.clone(),
        name: p.name,
        tags: p.tags.clone(),
        connected_at: Some(now),
        last_seen_at: Some(now),
        capacity_max: p.capacity_max,
    };
    db_comm::upsert_agent(pool, &agent)?;

    // Reset any in-progress tasks from a previous session.
    let reset = db_tasks::reset_in_progress_for_agent(pool, &agent.id)?;
    if reset > 0 {
        info!(
            "Agent '{}' registered: reset {} orphaned in-progress task(s) to pending",
            agent.id, reset
        );
    }

    // Update registry entry with real tags, capacity_max, and mark as registered.
    // Setting registered=true enables proactive dispatch for this agent.
    if let Ok(mut agents) = registry.lock() {
        if let Some(state) = agents.get_mut(&p.id) {
            state.tags = p.tags;
            state.capacity_max = p.capacity_max;
            state.active_tasks = 0;
            state.last_seen_at = now;
            state.registered = true;
        }
    }

    // Dispatch is triggered from ws.rs after the response is sent,
    // so the agent receives the response before any task.assign push.

    Ok(serde_json::json!({ "ok": true, "agent_id": agent.id }))
}

/// Handle `agent.status { active_tasks }`.
///
/// Updates in-memory registry. Triggers `try_dispatch` when the agent has
/// capacity (`active_tasks` dropped below `capacity_max`).
pub fn status(registry: &AgentRegistry, _pool: &DbPool, agent_id: &str, params: Option<Value>) -> Result<Value> {
    let p: StatusParams = serde_json::from_value(params.unwrap_or(Value::Null))?;

    let mut agents = registry.lock().map_err(|_| anyhow::anyhow!("registry lock poisoned"))?;
    let state = agents
        .get_mut(agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent not found in registry"))?;
    state.active_tasks = p.active_tasks;
    state.last_seen_at = Utc::now();

    // Dispatch is triggered from ws.rs after the response is sent,
    // so the agent receives the response before any task.assign push.
    Ok(serde_json::json!({ "ok": true }))
}
