//! Dev server lifecycle and observability command endpoints.

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::info;

use crate::process;
use crate::AppState;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub status: &'static str,
    pub output: String,
    pub exit_code: i32,
}

#[derive(Debug, Deserialize, Default)]
pub struct TestRequest {
    pub pattern: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct LogsQuery {
    pub tail: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct StdinRequest {
    pub input: String,
}

// ── Dev server lifecycle (long-running, tracked) ──────────────────────────────

pub async fn start(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cmd = state
        .exec_config
        .commands
        .get("start")
        .cloned()
        .unwrap_or_else(|| "npm run dev".to_string());

    match process::start(&state.processes, "dev", &cmd).await {
        Ok(info) => Json(serde_json::json!({
            "status": "ok",
            "pid": info.pid,
            "command": info.command,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e,
        })),
    }
}

pub async fn stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    match process::stop(&state.processes, "dev").await {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e,
        })),
    }
}

pub async fn restart(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Stop (ignore error if nothing running).
    let _ = process::stop(&state.processes, "dev").await;

    let cmd = state
        .exec_config
        .commands
        .get("start")
        .cloned()
        .unwrap_or_else(|| "npm run dev".to_string());

    match process::start(&state.processes, "dev", &cmd).await {
        Ok(info) => Json(serde_json::json!({
            "status": "ok",
            "pid": info.pid,
            "command": info.command,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e,
        })),
    }
}

pub async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let st = process::status(&state.processes, "dev").await;
    Json(serde_json::to_value(st).unwrap())
}

pub async fn logs(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> Json<serde_json::Value> {
    match process::get_logs(&state.processes, "dev", q.tail).await {
        Ok((output, line_count)) => Json(serde_json::json!({
            "output": output,
            "line_count": line_count,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e,
        })),
    }
}

pub async fn stdin(State(state): State<AppState>, Json(req): Json<StdinRequest>) -> Json<serde_json::Value> {
    match process::send_stdin(&state.processes, "dev", &req.input).await {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e,
        })),
    }
}

// ── Synchronous observability commands ────────────────────────────────────────

pub async fn test(
    State(state): State<AppState>,
    body: Option<Json<TestRequest>>,
) -> Json<CommandResponse> {
    let base = state
        .exec_config
        .commands
        .get("test")
        .cloned()
        .unwrap_or_else(|| "npm test".to_string());
    let pattern = body.and_then(|b| b.0.pattern).unwrap_or_default();
    let cmd = if pattern.is_empty() {
        base
    } else {
        format!("{base} {}", shell_escape(&pattern))
    };
    Json(run_command(&cmd).await)
}

pub async fn check(State(state): State<AppState>) -> Json<CommandResponse> {
    let cmd = state
        .exec_config
        .commands
        .get("check")
        .cloned()
        .unwrap_or_else(|| "npm run check".to_string());
    Json(run_command(&cmd).await)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Minimal shell escaping: wrap in single quotes and escape any single quotes inside.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

async fn run_command(cmd: &str) -> CommandResponse {
    info!("{}", cmd);
    let output = Command::new("sh").arg("-c").arg(cmd).output().await;

    match output {
        Err(e) => CommandResponse {
            status: "error",
            output: format!("failed to spawn: {e}"),
            exit_code: -1,
        },
        Ok(out) => {
            let exit_code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = if stderr.is_empty() {
                stdout.into_owned()
            } else {
                format!("{stdout}{stderr}")
            };
            CommandResponse {
                status: if out.status.success() { "ok" } else { "error" },
                output: combined,
                exit_code,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shell_escape;

    #[test]
    fn test_shell_escape_plain_string() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's a test"), r"'it'\''s a test'");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_multiple_quotes() {
        assert_eq!(shell_escape("a'b'c"), r"'a'\''b'\''c'");
    }
}
