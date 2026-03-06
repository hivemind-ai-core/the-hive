//! Agent struct: owns the polling loop and task execution lifecycle.

use std::sync::Arc;

use hive_core::types::{PushMessage, Task};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::client::{self, ClientCmd, PendingRequests};

/// Shared agent state passed into closures and spawned tasks.
pub struct Agent {
    pub agent_id: Arc<String>,
    pub agent_tags: Vec<String>,
    pub coding_agent: Arc<String>,
    pub cmd_tx: UnboundedSender<ClientCmd>,
    pub pending: PendingRequests,
}

impl Agent {
    pub fn new(
        agent_id: String,
        agent_tags: Vec<String>,
        coding_agent: String,
        cmd_tx: UnboundedSender<ClientCmd>,
        pending: PendingRequests,
    ) -> Self {
        Self {
            agent_id: Arc::new(agent_id),
            agent_tags,
            coding_agent: Arc::new(coding_agent),
            cmd_tx,
            pending,
        }
    }

    /// Spawn the polling loop. Returns immediately; runs in background.
    pub fn spawn_polling(&self) {
        let agent_id = Arc::clone(&self.agent_id);
        let agent_tags = self.agent_tags.clone();
        let cmd_tx = self.cmd_tx.clone();
        let pending = self.pending.clone();
        let coding_agent = Arc::clone(&self.coding_agent);

        tokio::spawn(async move {
            crate::polling::run(
                (*agent_id).clone(),
                agent_tags,
                (*coding_agent).clone(),
                cmd_tx.clone(),
                pending.clone(),
                move |task| Self::spawn_task(task, Arc::clone(&agent_id), Arc::clone(&coding_agent), cmd_tx.clone(), pending.clone()),
            )
            .await;
        });
    }

    fn spawn_task(
        task: Task,
        agent_id: Arc<String>,
        coding_agent: Arc<String>,
        cmd_tx: UnboundedSender<ClientCmd>,
        pending: PendingRequests,
    ) {
        tokio::spawn(async move {
            info!("Running task: {} ({})", task.title, task.id);

            // Fetch undelivered push messages before starting the task.
            let push_req = client::request(
                "push.list",
                Some(serde_json::json!({ "agent_id": *agent_id })),
            );
            let message_ids: Vec<String>;
            let messages: Vec<PushMessage> =
                match crate::polling::send_request(&cmd_tx, &pending, push_req).await {
                    Some(resp) => {
                        let msgs: Vec<PushMessage> = resp.result
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
                    crate::session::clear(&agent_id);
                    Some(format!("error: {e}"))
                }
            };

            // Acknowledge the push messages that were included in the prompt.
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
        });
    }
}
