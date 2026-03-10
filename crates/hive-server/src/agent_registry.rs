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
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::DbPool;
use crate::tasks as db_tasks;

/// Per-agent in-memory state for a connected agent.
pub struct AgentState {
    pub(crate) id: String,
    pub(crate) tags: Vec<String>,
    /// Maximum concurrent tasks this agent can run (sent in agent.register).
    pub(crate) capacity_max: u8,
    /// Current number of active tasks (updated by agent.status messages).
    pub(crate) active_tasks: u8,
    pub(crate) last_seen_at: DateTime<Utc>,
    /// Channel to send WebSocket messages to this agent.
    pub(crate) ws_tx: UnboundedSender<Message>,
    /// True after agent.register has been processed. Only registered agents
    /// are eligible for proactive dispatch. This maintains backward compatibility
    /// with old-style agents that use `task.get_next` directly.
    pub(crate) registered: bool,
}

impl AgentState {
    /// Create a new `AgentState` with default values.
    /// `registered` is false — set to true after `agent.register` is processed.
    pub fn new(id: String, ws_tx: UnboundedSender<Message>) -> Self {
        Self {
            id,
            tags: vec![],
            capacity_max: 1,
            active_tasks: 0,
            last_seen_at: Utc::now(),
            ws_tx,
            registered: false,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn registered(&self) -> bool {
        self.registered
    }

    /// Returns true when the agent can accept another task.
    pub fn has_capacity(&self) -> bool {
        self.active_tasks < self.capacity_max
    }
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
/// Call from: agent.register, agent.status (when `active_tasks` drops), task creation.
pub fn try_dispatch(registry: &AgentRegistry, db: &DbPool) -> bool {
    let mut agents = match registry.lock() {
        Ok(g) => g,
        Err(e) => {
            warn!("AgentRegistry lock poisoned: {e}");
            return false;
        }
    };

    // Collect eligible agents — registered and with available capacity.
    let eligible: Vec<(String, Option<String>)> = agents
        .values()
        .filter(|a| a.registered() && a.has_capacity())
        .map(|a| (a.id().to_string(), a.tags().first().cloned()))
        .collect();

    if eligible.is_empty() {
        return false;
    }

    // Try each eligible agent until we find one that can be matched to a pending task.
    for (agent_id, first_tag) in &eligible {
        let tag = first_tag.as_deref();

        let task = match db_tasks::get_next(db, agent_id, tag) {
            Ok(Some(t)) => t,
            Ok(None) => continue,
            Err(e) => {
                warn!("try_dispatch DB error: {e}");
                continue;
            }
        };

        // Update registry and send task.assign push.
        if let Some(state) = agents.get_mut(agent_id.as_str()) {
            state.active_tasks += 1;
            let msg = make_task_assign(&task);
            if let Ok(json) = serde_json::to_string(&msg) {
                if let Err(e) = state.ws_tx.send(Message::Text(json.into())) {
                    debug!(agent_id = %agent_id, method = "task.assign", error = %e, "ws_tx send failed; agent likely disconnected");
                }
            }
        }

        info!(
            "Dispatched task '{}' ({}) to agent '{}'",
            task.title, task.id, agent_id
        );
        return true;
    }

    false
}

/// Send a `push.notify` message to a specific agent (if connected).
///
/// Called when new push messages arrive for `agent_id` — for example when the agent
/// is `@mentioned` in a topic comment. Silently does nothing if the agent is not
/// currently connected (messages are persisted in the DB for offline delivery).
pub fn notify_agent(registry: &AgentRegistry, agent_id: &str, messages: &serde_json::Value) {
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
                if let Err(e) = state.ws_tx.send(Message::Text(json.into())) {
                    debug!(agent_id = %agent_id, method = "push.notify", error = %e, "ws_tx send failed; agent likely disconnected");
                }
            }
        }
    }
}

/// Broadcast a message to all currently connected agents.
///
/// Used for state-change notifications (`agents.updated`, `tasks.updated`,
/// `topics.updated`) so every agent can refresh its local view.
/// Per-agent send failures are logged at debug level and do not abort the broadcast.
pub fn broadcast_all(registry: &AgentRegistry, msg: &ApiMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        if let Ok(agents) = registry.lock() {
            for state in agents.values() {
                if let Err(e) = state.ws_tx.send(Message::Text(json.clone().into())) {
                    debug!(agent_id = %state.id, method = ?msg.method, error = %e, "ws_tx send failed; agent likely disconnected");
                }
            }
        }
    }
}

/// Send a message to one specific agent.
///
/// Unlike [`notify_agent`], this delivers any [`ApiMessage`] (not just push notifications).
/// Used by handlers to respond to requests that need out-of-band delivery, or to
/// push targeted notifications. Silently does nothing if the agent is not connected.
pub fn send_to_agent(registry: &AgentRegistry, agent_id: &str, msg: &ApiMessage) {
    if let Ok(agents) = registry.lock() {
        if let Some(state) = agents.get(agent_id) {
            if let Ok(json) = serde_json::to_string(msg) {
                if let Err(e) = state.ws_tx.send(Message::Text(json.into())) {
                    debug!(agent_id = %agent_id, method = ?msg.method, error = %e, "ws_tx send failed; agent likely disconnected");
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::tasks as db_tasks;
    use crate::test_helpers::*;
    use tokio::sync::mpsc;

    fn open_test_db() -> crate::db::DbPool {
        let pool = db::open(":memory:").unwrap();
        db::run_migrations(&pool).unwrap();
        pool
    }

    fn make_agent_state(id: &str) -> (AgentState, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = AgentState::new(id.to_string(), tx);
        (state, rx)
    }

    // ── AgentState ──────────────────────────────────────────────────────────

    #[test]
    fn agent_state_new_defaults() {
        let (state, _rx) = make_agent_state("agent-1");
        assert_eq!(state.id(), "agent-1");
        assert!(state.tags().is_empty());
        assert_eq!(state.capacity_max, 1);
        assert_eq!(state.active_tasks, 0);
        assert!(!state.registered());
        assert!(state.has_capacity());
    }

    #[test]
    fn agent_state_has_capacity_when_below_max() {
        let (mut state, _rx) = make_agent_state("agent-1");
        state.capacity_max = 2;
        state.active_tasks = 1;
        assert!(state.has_capacity());
    }

    #[test]
    fn agent_state_no_capacity_at_max() {
        let (mut state, _rx) = make_agent_state("agent-1");
        state.capacity_max = 1;
        state.active_tasks = 1;
        assert!(!state.has_capacity());
    }

    #[test]
    fn agent_state_no_capacity_above_max() {
        let (mut state, _rx) = make_agent_state("agent-1");
        state.capacity_max = 1;
        state.active_tasks = 3;
        assert!(!state.has_capacity());
    }

    // ── new_registry ────────────────────────────────────────────────────────

    #[test]
    fn new_registry_is_empty() {
        let reg = new_registry();
        let agents = reg.lock().unwrap();
        assert!(agents.is_empty());
    }

    // ── try_dispatch ────────────────────────────────────────────────────────

    #[test]
    fn try_dispatch_no_agents_returns_false() {
        let registry = new_registry();
        let pool = open_test_db();
        db_tasks::insert_task(&pool, &make_task("Pending task")).unwrap();
        assert!(!try_dispatch(&registry, &pool));
    }

    #[test]
    fn try_dispatch_no_tasks_returns_false() {
        let registry = new_registry();
        let pool = open_test_db();
        let (mut state, _rx) = make_agent_state("agent-1");
        state.registered = true;
        registry.lock().unwrap().insert("agent-1".to_string(), state);
        assert!(!try_dispatch(&registry, &pool));
    }

    #[test]
    fn try_dispatch_unregistered_agent_skipped() {
        let registry = new_registry();
        let pool = open_test_db();
        db_tasks::insert_task(&pool, &make_task("Pending task")).unwrap();
        // Agent is NOT registered — should be skipped.
        let (state, _rx) = make_agent_state("agent-1");
        registry.lock().unwrap().insert("agent-1".to_string(), state);
        assert!(!try_dispatch(&registry, &pool));
    }

    #[test]
    fn try_dispatch_agent_at_capacity_skipped() {
        let registry = new_registry();
        let pool = open_test_db();
        db_tasks::insert_task(&pool, &make_task("Pending task")).unwrap();
        let (mut state, _rx) = make_agent_state("agent-1");
        state.registered = true;
        state.active_tasks = 1; // at capacity (max=1)
        registry.lock().unwrap().insert("agent-1".to_string(), state);
        assert!(!try_dispatch(&registry, &pool));
    }

    #[test]
    fn try_dispatch_sends_task_assign() {
        let registry = new_registry();
        let pool = open_test_db();
        db_tasks::insert_task(&pool, &make_task("Dispatchable")).unwrap();
        let (mut state, mut rx) = make_agent_state("agent-1");
        state.registered = true;
        registry.lock().unwrap().insert("agent-1".to_string(), state);

        assert!(try_dispatch(&registry, &pool));

        // Verify task.assign message was sent
        let msg = rx.try_recv().unwrap();
        if let Message::Text(text) = msg {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["method"], "task.assign");
            assert_eq!(v["params"]["task"]["title"], "Dispatchable");
        } else {
            panic!("expected text message");
        }

        // Verify active_tasks incremented
        let agents = registry.lock().unwrap();
        assert_eq!(agents["agent-1"].active_tasks, 1);
    }

    #[test]
    fn try_dispatch_tag_matching() {
        let registry = new_registry();
        let pool = open_test_db();

        // Create a rust-tagged task
        db_tasks::insert_task(&pool, &make_tagged_task("Rust Task", &["rust"])).unwrap();

        // Agent with "python" tag — should NOT match the "rust" task.
        let (mut state, _rx) = make_agent_state("python-agent");
        state.registered = true;
        state.tags = vec!["python".to_string()];
        registry.lock().unwrap().insert("python-agent".to_string(), state);

        assert!(!try_dispatch(&registry, &pool));
    }

    #[test]
    fn try_dispatch_iterates_all_agents() {
        let registry = new_registry();
        let pool = open_test_db();

        // Create a python-tagged task
        db_tasks::insert_task(&pool, &make_tagged_task("Python Task", &["python"])).unwrap();

        // First agent: rust tag (won't match)
        let (mut rust_state, _rx1) = make_agent_state("rust-agent");
        rust_state.registered = true;
        rust_state.tags = vec!["rust".to_string()];

        // Second agent: python tag (will match)
        let (mut python_state, mut rx2) = make_agent_state("python-agent");
        python_state.registered = true;
        python_state.tags = vec!["python".to_string()];

        {
            let mut agents = registry.lock().unwrap();
            agents.insert("rust-agent".to_string(), rust_state);
            agents.insert("python-agent".to_string(), python_state);
        }

        assert!(try_dispatch(&registry, &pool));

        // Python agent should have received the task
        let msg = rx2.try_recv().unwrap();
        if let Message::Text(text) = msg {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["params"]["task"]["title"], "Python Task");
        } else {
            panic!("expected text message");
        }
    }

    // ── notify_agent ────────────────────────────────────────────────────────

    #[test]
    fn notify_agent_sends_push_notify() {
        let registry = new_registry();
        let (state, mut rx) = make_agent_state("agent-1");
        registry.lock().unwrap().insert("agent-1".to_string(), state);

        let messages = serde_json::json!([{"content": "hello"}]);
        notify_agent(&registry, "agent-1", &messages);

        let msg = rx.try_recv().unwrap();
        if let Message::Text(text) = msg {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["type"], "push");
            assert_eq!(v["method"], "push.notify");
            assert_eq!(v["params"]["messages"][0]["content"], "hello");
        } else {
            panic!("expected text message");
        }
    }

    #[test]
    fn notify_agent_does_nothing_for_unknown_agent() {
        let registry = new_registry();
        let messages = serde_json::json!([{"content": "hello"}]);
        // Should not panic
        notify_agent(&registry, "nonexistent", &messages);
    }

    // ── broadcast_all ───────────────────────────────────────────────────────

    #[test]
    fn broadcast_all_sends_to_all_agents() {
        let registry = new_registry();
        let (state1, mut rx1) = make_agent_state("agent-1");
        let (state2, mut rx2) = make_agent_state("agent-2");
        {
            let mut agents = registry.lock().unwrap();
            agents.insert("agent-1".to_string(), state1);
            agents.insert("agent-2".to_string(), state2);
        }

        let msg = ApiMessage {
            msg_type: MessageType::Push,
            id: "test".to_string(),
            method: Some("tasks.updated".to_string()),
            params: Some(serde_json::json!([])),
            result: None,
            error: None,
        };
        broadcast_all(&registry, &msg);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn broadcast_all_empty_registry_is_ok() {
        let registry = new_registry();
        let msg = ApiMessage {
            msg_type: MessageType::Push,
            id: "test".to_string(),
            method: Some("test".to_string()),
            params: None,
            result: None,
            error: None,
        };
        // Should not panic
        broadcast_all(&registry, &msg);
    }

    // ── send_to_agent ───────────────────────────────────────────────────────

    #[test]
    fn send_to_agent_delivers_to_correct_agent() {
        let registry = new_registry();
        let (state1, mut rx1) = make_agent_state("agent-1");
        let (state2, mut rx2) = make_agent_state("agent-2");
        {
            let mut agents = registry.lock().unwrap();
            agents.insert("agent-1".to_string(), state1);
            agents.insert("agent-2".to_string(), state2);
        }

        let msg = ApiMessage {
            msg_type: MessageType::Response,
            id: "test".to_string(),
            method: None,
            params: None,
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        };
        send_to_agent(&registry, "agent-1", &msg);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_err()); // agent-2 should not receive
    }

    #[test]
    fn send_to_agent_unknown_is_noop() {
        let registry = new_registry();
        let msg = ApiMessage {
            msg_type: MessageType::Response,
            id: "test".to_string(),
            method: None,
            params: None,
            result: None,
            error: None,
        };
        // Should not panic
        send_to_agent(&registry, "nonexistent", &msg);
    }

    // ── make_task_assign ────────────────────────────────────────────────────

    #[test]
    fn make_task_assign_has_correct_shape() {
        let task = make_task("Test Task");
        let msg = make_task_assign(&task);
        assert_eq!(msg.msg_type, MessageType::Push);
        assert_eq!(msg.method.as_deref(), Some("task.assign"));
        assert_eq!(msg.params.as_ref().unwrap()["task"]["title"], "Test Task");
    }
}
