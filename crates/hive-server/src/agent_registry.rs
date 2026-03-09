//! In-memory agent state registry for the v2 communication protocol.
//!
//! The registry is the single source of truth for which agents are connected,
//! how busy they are, and how to reach them. `try_dispatch` is the only place
//! that moves tasks from `pending` to `in-progress`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::Message;
use chrono::{DateTime, Utc};
use hive_core::types::{ApiMessage, MessageType, Task};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::DbPool;
use crate::tasks as db_tasks;

/// Per-agent in-memory state for a connected agent.
pub struct AgentState {
    pub id: String,
    pub tags: Vec<String>,
    /// Maximum concurrent tasks this agent can run (sent in agent.register).
    pub capacity_max: u8,
    /// Current number of active tasks (updated by agent.status messages).
    pub active_tasks: u8,
    pub last_seen_at: DateTime<Utc>,
    /// Channel to send WebSocket messages to this agent.
    pub ws_tx: UnboundedSender<Message>,
    /// True after agent.register has been processed. Only registered agents
    /// are eligible for proactive dispatch. This maintains backward compatibility
    /// with old-style agents that use task.get_next directly.
    pub registered: bool,
}

/// Thread-safe registry of all connected agents.
pub type AgentRegistry = Arc<Mutex<HashMap<String, AgentState>>>;

pub fn new_registry() -> AgentRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Try to dispatch a pending task to an eligible idle agent.
///
/// Acquires the registry lock for the duration of the operation (including the
/// DB claim) to prevent double-assignment. Returns true if a task was dispatched.
///
/// Call from: agent.register, agent.status (when active_tasks drops), task creation.
pub fn try_dispatch(registry: &AgentRegistry, db: &DbPool) -> bool {
    let mut agents = match registry.lock() {
        Ok(g) => g,
        Err(e) => {
            warn!("AgentRegistry lock poisoned: {e}");
            return false;
        }
    };

    // Find the first registered agent that has available capacity.
    // Only agents that sent agent.register are eligible for proactive dispatch.
    let agent_info = agents
        .values()
        .filter(|a| a.registered && a.active_tasks < a.capacity_max)
        .map(|a| (a.id.clone(), a.tags.first().cloned()))
        .next();

    let Some((agent_id, first_tag)) = agent_info else {
        return false;
    };

    let tag = first_tag.as_deref();

    // Atomically claim a pending task in the DB.
    let task = match db_tasks::get_next(db, &agent_id, tag) {
        Ok(Some(t)) => t,
        Ok(None) => return false,
        Err(e) => {
            warn!("try_dispatch DB error: {e}");
            return false;
        }
    };

    // Update registry and send task.assign push.
    if let Some(state) = agents.get_mut(&agent_id) {
        state.active_tasks += 1;
        let msg = make_task_assign(&task);
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = state.ws_tx.send(Message::Text(json.into()));
        }
    }

    info!(
        "Dispatched task '{}' ({}) to agent '{}'",
        task.title, task.id, agent_id
    );
    true
}

/// Send a `push.notify` message to a specific agent (if connected).
pub fn notify_agent(registry: &AgentRegistry, agent_id: &str, messages: serde_json::Value) {
    if let Ok(agents) = registry.lock() {
        if let Some(state) = agents.get(agent_id) {
            let msg = ApiMessage {
                msg_type: MessageType::Push,
                id: Uuid::new_v4().to_string(),
                method: Some("push.notify".to_string()),
                params: Some(serde_json::json!({ "messages": messages })),
                result: None,
                error: None,
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = state.ws_tx.send(Message::Text(json.into()));
            }
        }
    }
}

/// Broadcast a message to all connected agents.
pub fn broadcast_all(registry: &AgentRegistry, msg: &ApiMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        if let Ok(agents) = registry.lock() {
            for state in agents.values() {
                let _ = state.ws_tx.send(Message::Text(json.clone().into()));
            }
        }
    }
}

/// Send a message to one specific agent.
pub fn send_to_agent(registry: &AgentRegistry, agent_id: &str, msg: &ApiMessage) {
    if let Ok(agents) = registry.lock() {
        if let Some(state) = agents.get(agent_id) {
            if let Ok(json) = serde_json::to_string(msg) {
                let _ = state.ws_tx.send(Message::Text(json.into()));
            }
        }
    }
}

fn make_task_assign(task: &Task) -> ApiMessage {
    ApiMessage {
        msg_type: MessageType::Push,
        id: Uuid::new_v4().to_string(),
        method: Some("task.assign".to_string()),
        params: Some(serde_json::json!({ "task": task })),
        result: None,
        error: None,
    }
}
