//! Session ID persistence for coding agent continuity.
//!
//! Session files are stored at `/app/.hive/agents/{agent_id}/session`.
//! They contain the session ID returned by the previous coding agent run,
//! allowing the next run to resume from where it left off.

use std::path::PathBuf;

use anyhow::Result;
use tracing::{info, warn};

fn session_path(agent_id: &str) -> PathBuf {
    PathBuf::from(format!("/app/.hive/agents/{agent_id}/session"))
}

/// Load the stored session ID for this agent, if any.
pub fn load(agent_id: &str) -> Option<String> {
    let path = session_path(agent_id);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let id = s.trim().to_string();
            if id.is_empty() {
                None
            } else {
                info!("Loaded session id for {agent_id}: {id}");
                Some(id)
            }
        }
        Err(_) => None,
    }
}

/// Save a session ID for future resumption.
pub fn save(agent_id: &str, session_id: &str) -> Result<()> {
    let path = session_path(agent_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, session_id)?;
    info!("Saved session id for {agent_id}: {session_id}");
    Ok(())
}

/// Clear the stored session (e.g. on fatal agent error).
pub fn clear(agent_id: &str) {
    let path = session_path(agent_id);
    if let Err(e) = std::fs::remove_file(&path) {
        warn!("Could not clear session for {agent_id}: {e}");
    }
}

/// Extract a session ID from coding agent output.
///
/// Handles multiple formats:
/// - Kilo: `Session: <id>`
/// - Claude Code plain text: `Session ID: <id>`
/// - Claude Code JSON stream: `{"sessionId":"<id>",...}` or `{"session_id":"<id>",...}`
pub fn extract_from_output(output: &str) -> Option<String> {
    let mut last_json_id: Option<String> = None;

    for line in output.lines() {
        let line = line.trim();

        // Plain text patterns.
        if let Some(id) = line.strip_prefix("Session: ") {
            return Some(id.trim().to_string());
        }
        if let Some(id) = line.strip_prefix("Session ID: ") {
            return Some(id.trim().to_string());
        }
        if let Some(id) = line.strip_prefix("session-id: ") {
            return Some(id.trim().to_string());
        }

        // JSON stream pattern — try to parse each line as JSON.
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let id = v.get("sessionId")
                    .or_else(|| v.get("session_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if id.is_some() {
                    last_json_id = id;
                }
            }
        }
    }

    last_json_id
}

