//! Agent struct: owns task execution lifecycle.

use std::sync::{
    Arc,
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
        }
    }

    /// Called when the server pushes `task.assign { task }`. Spawns execution.
    pub fn on_task_assign(&self, task: Task) {
        Self::spawn_task(
            task,
            Arc::clone(&self.agent_id),
            Arc::clone(&self.coding_agent),
            self.cmd_tx.clone(),
            self.pending.clone(),
            Arc::clone(&self.active_tasks),
            self.last_status.clone(),
        );
    }

    fn spawn_task(
        task: Task,
        agent_id: Arc<String>,
        coding_agent: Arc<String>,
        cmd_tx: UnboundedSender<ClientCmd>,
        pending: PendingRequests,
        active_tasks: Arc<AtomicU8>,
        last_status: LastStatus,
    ) {
        tokio::spawn(async move {
            info!("Running task: {} ({})", task.title, task.id);

            // Report busy immediately.
            let n = active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
            status::report(&cmd_tx, n, &last_status);

            // Fetch any undelivered push messages so they're included in the prompt.
            let push_req = client::request(
                "push.list",
                Some(serde_json::json!({ "agent_id": *agent_id })),
            );
            let message_ids: Vec<String>;
            let messages: Vec<PushMessage> =
                match client::send_request(&cmd_tx, &pending, push_req).await {
                    Some(resp) => {
                        let msgs: Vec<PushMessage> = resp
                            .result
                            .and_then(|v| serde_json::from_value(v).ok())
                            .unwrap_or_default();
                        message_ids = msgs.iter().map(|m| m.id.clone()).collect();
                        msgs
                    }
                    None => {
                        warn!("Failed to fetch push messages for task {}", task.id);
                        message_ids = vec![];
                        vec![]
                    }
                };

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

            // Report idle after completing.
            let n = active_tasks.fetch_sub(1, Ordering::SeqCst) - 1;
            status::report(&cmd_tx, n, &last_status);
        });
    }
}
