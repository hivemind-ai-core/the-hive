//! Agent status reporting and watchdog heartbeat.
//!
//! `report` sends `agent.status { active_tasks }` and timestamps the send.
//! `spawn_watchdog` sends `agent.heartbeat` when the agent has been silent
//! for `WATCHDOG_INTERVAL`, keeping the server's `last_seen_at` fresh.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info};

use crate::client::{request, ClientCmd};

/// How long of silence triggers a watchdog heartbeat.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);
/// How often the watchdog loop wakes to check elapsed time.
const WATCHDOG_CHECK: Duration = Duration::from_secs(10);

/// Shared timestamp of the last status or heartbeat sent.
pub type LastStatus = Arc<Mutex<Instant>>;

pub fn new_last_status() -> LastStatus {
    Arc::new(Mutex::new(Instant::now()))
}

/// Send `agent.status { active_tasks }` and record the send time.
pub fn report(cmd_tx: &UnboundedSender<ClientCmd>, active_tasks: u8, last_status: &LastStatus) {
    let msg = request(
        "agent.status",
        Some(serde_json::json!({ "active_tasks": active_tasks })),
    );
    if cmd_tx.send(ClientCmd::Send(msg)).is_ok() {
        if let Ok(mut t) = last_status.lock() {
            *t = Instant::now();
        }
        info!("agent.status sent: active_tasks={active_tasks}");
    }
}

/// Spawn the watchdog loop. Sends `agent.heartbeat` whenever no status has
/// been reported for `WATCHDOG_INTERVAL`, keeping `last_seen_at` current.
pub fn spawn_watchdog(cmd_tx: UnboundedSender<ClientCmd>, last_status: LastStatus) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(WATCHDOG_CHECK).await;
            let elapsed = last_status
                .lock()
                .map(|t| t.elapsed())
                .unwrap_or_default();
            if elapsed >= WATCHDOG_INTERVAL {
                let msg = request("agent.heartbeat", None);
                if cmd_tx.send(ClientCmd::Send(msg)).is_err() {
                    break;
                }
                if let Ok(mut t) = last_status.lock() {
                    *t = Instant::now();
                }
                debug!("watchdog heartbeat sent");
            }
        }
    });
}
