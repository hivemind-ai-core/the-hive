//! Dev server and observability command endpoints.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::info;

use crate::exec::ExecConfig;

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

pub async fn start(State(cfg): State<ExecConfig>) -> Json<CommandResponse> {
    let cmd = cfg.commands.get("start").cloned().unwrap_or_else(|| "npm run dev".to_string());
    Json(run_command(&cmd).await)
}

pub async fn stop(State(cfg): State<ExecConfig>) -> Json<CommandResponse> {
    let cmd = cfg.commands.get("stop").cloned().unwrap_or_else(|| "npm run stop".to_string());
    Json(run_command(&cmd).await)
}

pub async fn restart(State(cfg): State<ExecConfig>) -> Json<CommandResponse> {
    let cmd = cfg.commands.get("restart").cloned().unwrap_or_else(|| "npm run restart".to_string());
    Json(run_command(&cmd).await)
}

pub async fn test(
    State(cfg): State<ExecConfig>,
    body: Option<Json<TestRequest>>,
) -> Json<CommandResponse> {
    let base = cfg.commands.get("test").cloned().unwrap_or_else(|| "npm test".to_string());
    let pattern = body.and_then(|b| b.0.pattern).unwrap_or_default();
    let cmd = if pattern.is_empty() {
        base
    } else {
        format!("{base} {}", shell_escape(&pattern))
    };
    Json(run_command(&cmd).await)
}

pub async fn check(State(cfg): State<ExecConfig>) -> Json<CommandResponse> {
    let cmd = cfg.commands.get("check").cloned().unwrap_or_else(|| "npm run check".to_string());
    Json(run_command(&cmd).await)
}

pub async fn logs(State(cfg): State<ExecConfig>) -> Json<CommandResponse> {
    let cmd = cfg.commands.get("logs").cloned().unwrap_or_else(|| "npm run logs".to_string());
    Json(run_command(&cmd).await)
}

/// Minimal shell escaping: wrap in single quotes and escape any single quotes inside.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
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
        // "it's" → 'it'\''s'
        assert_eq!(shell_escape("it's a test"), r"'it'\''s a test'");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_multiple_quotes() {
        // "a'b'c" → 'a'\''b'\''c'
        assert_eq!(shell_escape("a'b'c"), r"'a'\''b'\''c'");
    }
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
