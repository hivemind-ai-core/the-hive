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

#[cfg(test)]
mod tests {
    use super::extract_from_output;

    #[test]
    fn test_kilo_format() {
        assert_eq!(extract_from_output("Session: abc123\nsome other output"), Some("abc123".to_string()));
    }

    #[test]
    fn test_claude_plain_text_format() {
        assert_eq!(extract_from_output("Session ID: xyz-789"), Some("xyz-789".to_string()));
    }

    #[test]
    fn test_old_format() {
        assert_eq!(extract_from_output("session-id: old-format-id"), Some("old-format-id".to_string()));
    }

    #[test]
    fn test_json_stream_session_id_field() {
        let output = r#"{"sessionId":"json-session-1","type":"result"}"#;
        assert_eq!(extract_from_output(output), Some("json-session-1".to_string()));
    }

    #[test]
    fn test_json_stream_snake_case_variant() {
        let output = r#"{"session_id":"json-session-2"}"#;
        assert_eq!(extract_from_output(output), Some("json-session-2".to_string()));
    }

    #[test]
    fn test_json_stream_last_one_wins() {
        let output = "first line\n{\"sessionId\":\"first\"}\n{\"sessionId\":\"second\"}\nlast line";
        assert_eq!(extract_from_output(output), Some("second".to_string()));
    }

    #[test]
    fn test_plain_text_short_circuits_json() {
        // Plain text `Session:` line before JSON — returns immediately without scanning JSON.
        let output = "Session: plain-wins\n{\"sessionId\":\"json-would-lose\"}";
        assert_eq!(extract_from_output(output), Some("plain-wins".to_string()));
    }

    #[test]
    fn test_no_session_returns_none() {
        assert_eq!(extract_from_output("No session here\nJust some output"), None);
    }

    #[test]
    fn test_json_without_session_key_returns_none() {
        let output = r#"{"type":"tool_use","name":"bash"}"#;
        assert_eq!(extract_from_output(output), None);
    }

    #[test]
    fn test_empty_string_returns_none() {
        assert_eq!(extract_from_output(""), None);
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

