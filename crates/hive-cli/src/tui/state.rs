//! Shared TUI state updated from the server.

use hive_core::types::{Agent, Task};

/// Lightweight task summary for display.
#[derive(Default, Clone)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub assigned: Option<String>,
}

impl From<&Task> for TaskSummary {
    fn from(t: &Task) -> Self {
        Self {
            id: t.id.clone(),
            title: t.title.clone(),
            status: format!("{:?}", t.status).to_lowercase().replace('"', ""),
            assigned: t.assigned_agent_id.clone(),
        }
    }
}

#[derive(Default)]
pub struct AppState {
    pub agents: Vec<Agent>,
    pub tasks: Vec<TaskSummary>,
    pub selected_task_idx: usize,
}
