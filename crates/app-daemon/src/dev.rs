//! Dev server and observability command endpoints.

use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::info;

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

pub async fn start() -> Json<CommandResponse> {
    Json(run_npm("run dev").await)
}

pub async fn stop() -> Json<CommandResponse> {
    Json(run_npm("run stop").await)
}

pub async fn restart() -> Json<CommandResponse> {
    Json(run_npm("run restart").await)
}

pub async fn test(body: Option<Json<TestRequest>>) -> Json<CommandResponse> {
    let pattern = body.and_then(|b| b.0.pattern).unwrap_or_default();
    let cmd = if pattern.is_empty() {
        "test".to_string()
    } else {
        // npm test -- <pattern>  (the -- separates npm args from test runner args)
        format!("test -- {}", shell_escape(&pattern))
    };
    Json(run_npm(&cmd).await)
}

pub async fn check() -> Json<CommandResponse> {
    Json(run_npm("run check").await)
}

pub async fn logs() -> Json<CommandResponse> {
    Json(run_npm("run logs").await)
}

/// Minimal shell escaping: wrap in single quotes and escape any single quotes inside.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

async fn run_npm(args: &str) -> CommandResponse {
    info!("npm {}", args);
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("npm {args}"))
        .output()
        .await;

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
