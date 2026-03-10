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
            let elapsed = last_status.lock().map(|t| t.elapsed()).unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientCmd;

    #[test]
    fn new_last_status_is_recent() {
        let ls = new_last_status();
        let elapsed = ls.lock().unwrap().elapsed();
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn report_sends_agent_status_message() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let last = new_last_status();

        // Make last_status "old"
        *last.lock().unwrap() = Instant::now() - Duration::from_secs(60);

        report(&cmd_tx, 2, &last);

        // Should have sent a message
        match cmd_rx.try_recv() {
            Ok(ClientCmd::Send(msg)) => {
                assert_eq!(msg.method.as_deref(), Some("agent.status"));
                assert_eq!(msg.params.as_ref().unwrap()["active_tasks"], 2);
            }
            other => panic!("expected Send, got: {other:?}"),
        }

        // last_status should be updated to recent
        let elapsed = last.lock().unwrap().elapsed();
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn report_with_zero_tasks() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let last = new_last_status();

        report(&cmd_tx, 0, &last);

        match cmd_rx.try_recv() {
            Ok(ClientCmd::Send(msg)) => {
                assert_eq!(msg.params.as_ref().unwrap()["active_tasks"], 0);
            }
            other => panic!("expected Send, got: {other:?}"),
        }
    }

    #[test]
    fn report_with_dropped_channel_does_not_panic() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<ClientCmd>();
        drop(cmd_rx); // close the receiver
        let last = new_last_status();
        // Should not panic
        report(&cmd_tx, 1, &last);
    }
}
