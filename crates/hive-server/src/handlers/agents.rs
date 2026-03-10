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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent_registry, db};
    use serde_json::json;
    use tokio::sync::mpsc;

    fn open_test_db() -> crate::db::DbPool {
        let pool = db::open(":memory:").unwrap();
        db::run_migrations(&pool).unwrap();
        pool
    }

    fn setup_registry_with_agent(id: &str) -> (crate::agent_registry::AgentRegistry, mpsc::UnboundedReceiver<axum::extract::ws::Message>) {
        let registry = agent_registry::new_registry();
        let (tx, rx) = mpsc::unbounded_channel();
        let state = agent_registry::AgentState::new(id.to_string(), tx);
        registry.lock().unwrap().insert(id.to_string(), state);
        (registry, rx)
    }

    // ── list handler ────────────────────────────────────────────────────────

    #[test]
    fn list_returns_empty_on_no_agents() {
        let pool = open_test_db();
        let result = list(&pool).unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }

    // ── register handler ────────────────────────────────────────────────────

    #[test]
    fn register_creates_agent_in_db() {
        let pool = open_test_db();
        let (registry, _rx) = setup_registry_with_agent("agent-1");

        let result = register(&pool, &registry, Some(json!({
            "id": "agent-1",
            "name": "Test Agent"
        }))).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["agent_id"], "agent-1");

        // Agent should be in DB
        let agents = list(&pool).unwrap();
        assert_eq!(agents.as_array().unwrap().len(), 1);
        assert_eq!(agents[0]["id"], "agent-1");
    }

    #[test]
    fn register_with_tags_and_capacity() {
        let pool = open_test_db();
        let (registry, _rx) = setup_registry_with_agent("agent-1");

        register(&pool, &registry, Some(json!({
            "id": "agent-1",
            "name": "Agent",
            "tags": ["rust", "backend"],
            "capacity_max": 3
        }))).unwrap();

        // Verify registry was updated
        let agents = registry.lock().unwrap();
        let state = agents.get("agent-1").unwrap();
        assert!(state.registered());
        assert_eq!(state.tags(), &["rust", "backend"]);
        assert_eq!(state.capacity_max, 3);
    }

    #[test]
    fn register_empty_id_rejected() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(register(&pool, &registry, Some(json!({"id": "", "name": "N"}))).is_err());
    }

    #[test]
    fn register_whitespace_id_rejected() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(register(&pool, &registry, Some(json!({"id": "  ", "name": "N"}))).is_err());
    }

    #[test]
    fn register_missing_id_rejected() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(register(&pool, &registry, Some(json!({"name": "N"}))).is_err());
    }

    #[test]
    fn register_missing_name_rejected() {
        let pool = open_test_db();
        let registry = agent_registry::new_registry();
        assert!(register(&pool, &registry, Some(json!({"id": "a"}))).is_err());
    }

    #[test]
    fn register_resets_orphaned_tasks() {
        let pool = open_test_db();
        let (registry, _rx) = setup_registry_with_agent("agent-1");

        // Create and claim a task
        let task = hive_core::types::Task::new("Orphan".to_string(), None, vec![]);
        crate::tasks::insert_task(&pool, &task).unwrap();
        crate::tasks::get_next(&pool, "agent-1", None).unwrap();

        // Verify it's in-progress
        let tasks = crate::tasks::list_tasks(&pool, Some("in-progress"), None, None).unwrap();
        assert_eq!(tasks.len(), 1);

        // Register resets orphaned tasks
        register(&pool, &registry, Some(json!({"id": "agent-1", "name": "Agent"}))).unwrap();

        // Task should be back to pending (or re-dispatched — but we verify not in-progress for this agent)
        let tasks = crate::tasks::list_tasks(&pool, Some("pending"), None, None).unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn register_default_capacity_is_1() {
        let pool = open_test_db();
        let (registry, _rx) = setup_registry_with_agent("agent-1");

        register(&pool, &registry, Some(json!({"id": "agent-1", "name": "Agent"}))).unwrap();

        let agents = registry.lock().unwrap();
        assert_eq!(agents["agent-1"].capacity_max, 1);
    }

    // ── status handler ──────────────────────────────────────────────────────

    #[test]
    fn status_updates_active_tasks() {
        let (registry, _rx) = setup_registry_with_agent("agent-1");
        let pool = open_test_db();

        let result = status(&registry, &pool, "agent-1", Some(json!({"active_tasks": 2}))).unwrap();
        assert_eq!(result["ok"], true);

        let agents = registry.lock().unwrap();
        assert_eq!(agents["agent-1"].active_tasks, 2);
    }

    #[test]
    fn status_unknown_agent_errors() {
        let registry = agent_registry::new_registry();
        let pool = open_test_db();
        assert!(status(&registry, &pool, "ghost", Some(json!({"active_tasks": 0}))).is_err());
    }

    #[test]
    fn status_missing_active_tasks_errors() {
        let (registry, _rx) = setup_registry_with_agent("agent-1");
        let pool = open_test_db();
        assert!(status(&registry, &pool, "agent-1", Some(json!({}))).is_err());
    }
}
