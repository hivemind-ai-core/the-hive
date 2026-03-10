//! Agent struct: owns task execution lifecycle.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use hive_core::types::{PushMessage, Task};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::client::{self, ClientCmd, PendingRequests};
use crate::status::{self, LastStatus};

/// Shared agent state. Clone-safe: all fields are either Arc or Clone.
#[derive(Clone)]
pub struct Agent {
    pub agent_id: Arc<String>,
    pub coding_agent: Arc<String>,
    pub cmd_tx: UnboundedSender<ClientCmd>,
    pub pending: PendingRequests,
    pub active_tasks: Arc<AtomicU8>,
    pub last_status: LastStatus,
    /// Push messages received while a task was executing. Drained at the start of the next task.
    pub push_cache: Arc<Mutex<Vec<PushMessage>>>,
}

impl Agent {
    pub fn new(
        agent_id: String,
        coding_agent: String,
        cmd_tx: UnboundedSender<ClientCmd>,
        pending: PendingRequests,
        last_status: LastStatus,
    ) -> Self {
        Self {
            agent_id: Arc::new(agent_id),
            coding_agent: Arc::new(coding_agent),
            cmd_tx,
            pending,
            active_tasks: Arc::new(AtomicU8::new(0)),
            last_status,
            push_cache: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Called when the server pushes `task.assign { task }`. Spawns execution.
    pub fn on_task_assign(&self, task: Task) {
        // Increment synchronously so push_rx sees the correct state immediately.
        let n = self.active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        status::report(&self.cmd_tx, n, &self.last_status);

        Self::spawn_task(self.clone(), task);
    }

    /// Called when `push.notify` arrives and agent is idle. Spawns a push-only execution.
    pub fn on_push_notify(&self, messages: Vec<PushMessage>) {
        if messages.is_empty() {
            return;
        }

        // Increment synchronously so subsequent push_rx messages see the correct state.
        let n = self.active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        status::report(&self.cmd_tx, n, &self.last_status);

        Self::spawn_push_only(self.clone(), messages);
    }

    fn spawn_task(agent: Agent, task: Task) {
        tokio::spawn(async move {
            let Agent {
                agent_id,
                coding_agent,
                cmd_tx,
                pending,
                active_tasks,
                last_status,
                push_cache,
            } = agent;

            info!("Running task: {} ({})", task.title, task.id);

            // active_tasks already incremented synchronously in on_task_assign.

            // Drain pre-cached messages (received via push.notify while a previous task ran).
            let cached: Vec<PushMessage> = push_cache
                .lock()
                .map(|mut c| c.drain(..).collect())
                .unwrap_or_default();

            // Fetch any remaining undelivered messages from the server.
            let push_req = client::request(
                "push.list",
                Some(serde_json::json!({ "agent_id": *agent_id })),
            );
            let server_msgs: Vec<PushMessage> =
                match client::send_request(&cmd_tx, &pending, push_req).await {
                    Some(resp) => resp
                        .result
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or_default(),
                    None => {
                        warn!("Failed to fetch push messages for task {}", task.id);
                        vec![]
                    }
                };

            // Merge, deduplicating by ID. Cached messages first (arrived chronologically first).
            let mut seen_ids = std::collections::HashSet::new();
            let messages: Vec<PushMessage> = cached
                .into_iter()
                .chain(server_msgs)
                .filter(|m| seen_ids.insert(m.id.clone()))
                .collect();
            let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();

            let result = crate::executor::run(&task, &coding_agent, &agent_id, &messages).await;
            let result_str = match result {
                Ok(r) => {
                    if r.exit_code != 0 {
                        crate::session::clear(&agent_id);
                    }
                    Some(r.output)
                }
                Err(e) => {
                    warn!("Executor error for task {}: {e}", task.id);
                    crate::session::clear(&agent_id);
                    Some(format!("error: {e}"))
                }
            };

            // Acknowledge push messages included in the prompt.
            if !message_ids.is_empty() {
                let ack_req = client::request(
                    "push.ack",
                    Some(serde_json::json!({ "message_ids": message_ids })),
                );
                if let Err(e) = cmd_tx.send(ClientCmd::Send(ack_req)) {
                    warn!("Failed to send push.ack for task {}: {e}", task.id);
                }
            }

            let complete_req = client::request(
                "task.complete",
                Some(serde_json::json!({ "id": task.id, "result": result_str })),
            );
            if let Err(e) = cmd_tx.send(ClientCmd::Send(complete_req)) {
                warn!("Failed to send task.complete for {}: {e}", task.id);
            }

            // Drain messages that arrived while this task was executing.
            let post_task: Vec<PushMessage> = push_cache
                .lock()
                .map(|mut c| c.drain(..).collect())
                .unwrap_or_default();

            if !post_task.is_empty() {
                info!(
                    "Processing {} push message(s) received during task execution",
                    post_task.len()
                );
                let post_ids: Vec<String> = post_task.iter().map(|m| m.id.clone()).collect();
                match crate::executor::run_push_only(&coding_agent, &agent_id, &post_task).await {
                    Ok(r) => info!("Post-task push-only finished (exit {})", r.exit_code),
                    Err(e) => warn!("Post-task push-only failed: {e}"),
                }
                let ack = client::request(
                    "push.ack",
                    Some(serde_json::json!({ "message_ids": post_ids })),
                );
                if let Err(e) = cmd_tx.send(ClientCmd::Send(ack)) {
                    warn!("Failed to send push.ack after post-task push-only: {e}");
                }
            }

            // Report idle after all work for this task cycle is complete.
            let n = active_tasks.fetch_sub(1, Ordering::SeqCst) - 1;
            status::report(&cmd_tx, n, &last_status);
        });
    }

    fn spawn_push_only(agent: Agent, messages: Vec<PushMessage>) {
        let Agent { agent_id, coding_agent, cmd_tx, active_tasks, last_status, .. } = agent;
        let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
        tokio::spawn(async move {
            // active_tasks already incremented synchronously in on_push_notify.
            match crate::executor::run_push_only(&coding_agent, &agent_id, &messages).await {
                Ok(r) => info!("Push-only execution finished (exit {})", r.exit_code),
                Err(e) => warn!("Push-only execution failed: {e}"),
            }

            // Ack all messages regardless of result.
            if !message_ids.is_empty() {
                let ack = client::request(
                    "push.ack",
                    Some(serde_json::json!({ "message_ids": message_ids })),
                );
                if let Err(e) = cmd_tx.send(ClientCmd::Send(ack)) {
                    warn!("Failed to send push.ack after push-only run: {e}");
                }
            }

            let n = active_tasks.fetch_sub(1, Ordering::SeqCst) - 1;
            status::report(&cmd_tx, n, &last_status);
        });
    }
}
