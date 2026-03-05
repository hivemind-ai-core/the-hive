//! Agent struct: owns the polling loop and task execution lifecycle.

use std::sync::Arc;

use hive_core::types::Task;
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
                cmd_tx.clone(),
                pending,
                move |task| Self::spawn_task(task, Arc::clone(&agent_id), Arc::clone(&coding_agent), cmd_tx.clone()),
            )
            .await;
        });
    }

    fn spawn_task(
        task: Task,
        agent_id: Arc<String>,
        coding_agent: Arc<String>,
        cmd_tx: UnboundedSender<ClientCmd>,
    ) {
        tokio::spawn(async move {
            info!("Running task: {} ({})", task.title, task.id);

            let result = crate::executor::run(&task, &coding_agent, &agent_id, &[]).await;
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
