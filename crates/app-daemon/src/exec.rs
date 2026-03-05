//! POST /exec endpoint — run a shell command and return its output.

use std::collections::HashMap;

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Allowlist config for the exec endpoint.
/// Mirrors the `[exec]` section of `.hive/config.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExecConfig {
    /// Exact aliases: "test" -> "pnpm test".
    pub commands: HashMap<String, String>,
    /// Allowed prefixes for `run <cmd>` (e.g. "cargo", "pnpm").
    pub run_prefixes: Vec<String>,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            commands: HashMap::from([
                ("test".to_string(), "pnpm test".to_string()),
                ("check".to_string(), "pnpm exec tsc --noEmit".to_string()),
                ("build".to_string(), "pnpm build".to_string()),
            ]),
            run_prefixes: vec![
                "cargo".to_string(),
                "npm".to_string(),
                "pnpm".to_string(),
            ],
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ExecRequest {
    /// The command to run (alias or `run <cmd>`).
    pub command: String,
    /// Optional pattern (e.g. test filter). Appended when present.
    pub pattern: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecResponse {
    pub status: &'static str,
    pub output: String,
    pub exit_code: i32,
}

/// Resolve `command` against the allowlist, returning the full shell command
/// to execute, or an error describing what is allowed.
fn resolve_command(command: &str, config: &ExecConfig) -> Result<String, String> {
    // 1. Exact alias match.
    if let Some(mapped) = config.commands.get(command) {
        return Ok(mapped.clone());
    }

    // 2. `run <cmd>` prefix allowlist.
    if let Some(inner) = command.strip_prefix("run ") {
        let inner = inner.trim();
        for prefix in &config.run_prefixes {
            if inner == prefix || inner.starts_with(&format!("{prefix} ")) {
                return Ok(inner.to_string());
            }
        }
        let allowed: Vec<&str> = config.run_prefixes.iter().map(|s| s.as_str()).collect();
        return Err(format!(
            "command not allowed; run_prefixes allowed: {}",
            allowed.join(", ")
        ));
    }

    // 3. No match.
    let aliases: Vec<&str> = config.commands.keys().map(|s| s.as_str()).collect();
    Err(format!(
        "unknown command '{}'; allowed aliases: {}; or use 'run <cmd>' with an allowed prefix",
        command,
        aliases.join(", ")
    ))
}

pub async fn exec(
    State(config): State<ExecConfig>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_command(&req.command, &config).map_err(|msg| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
    })?;

    let full_command = match req.pattern {
        Some(ref p) if !p.is_empty() => format!("{resolved} {p}"),
        _ => resolved,
    };

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&full_command);

    let output = cmd.output().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to spawn command: {e}") })),
        )
    })?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.is_empty() {
        stdout.into_owned()
    } else {
        format!("{stdout}{stderr}")
    };

    Ok(Json(ExecResponse {
        status: if output.status.success() { "ok" } else { "error" },
        output: combined,
        exit_code,
    }))
}
