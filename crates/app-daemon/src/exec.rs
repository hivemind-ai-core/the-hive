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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> ExecConfig {
        ExecConfig::default()
    }

    #[test]
    fn test_resolve_known_alias() {
        let cfg = default_cfg();
        assert_eq!(resolve_command("test", &cfg).unwrap(), "pnpm test");
        assert_eq!(resolve_command("build", &cfg).unwrap(), "pnpm build");
        assert_eq!(resolve_command("check", &cfg).unwrap(), "pnpm exec tsc --noEmit");
    }

    #[test]
    fn test_resolve_run_prefix_allowed() {
        let cfg = default_cfg();
        assert_eq!(resolve_command("run cargo test", &cfg).unwrap(), "cargo test");
        assert_eq!(resolve_command("run pnpm install", &cfg).unwrap(), "pnpm install");
        // Exact prefix match (no trailing args).
        assert_eq!(resolve_command("run cargo", &cfg).unwrap(), "cargo");
    }

    #[test]
    fn test_resolve_run_prefix_rejected() {
        let cfg = default_cfg();
        let err = resolve_command("run rm -rf /", &cfg).unwrap_err();
        assert!(err.contains("not allowed"), "rejected run cmd error: {err}");
    }

    #[test]
    fn test_resolve_unknown_command_errors() {
        let cfg = default_cfg();
        let err = resolve_command("deploy", &cfg).unwrap_err();
        assert!(err.contains("unknown command"), "error: {err}");
        assert!(err.contains("deploy"), "error should mention the command: {err}");
    }

    #[test]
    fn test_resolve_custom_alias_overrides_default() {
        let mut cfg = default_cfg();
        cfg.commands.insert("test".to_string(), "jest --ci".to_string());
        assert_eq!(resolve_command("test", &cfg).unwrap(), "jest --ci");
    }

    #[test]
    fn test_resolve_empty_command_errors() {
        let cfg = default_cfg();
        let err = resolve_command("", &cfg).unwrap_err();
        assert!(err.contains("unknown command"), "error: {err}");
    }

    #[test]
    fn test_resolve_run_prefix_near_match_rejected() {
        let cfg = default_cfg();
        // "carg" is a partial match for "cargo" but not a valid prefix.
        let err = resolve_command("run carg test", &cfg).unwrap_err();
        assert!(err.contains("not allowed"), "near-match should be rejected: {err}");
    }

    #[test]
    fn test_resolve_run_with_whitespace_only_inner() {
        let cfg = default_cfg();
        // "run " with nothing (or only whitespace) after → empty inner after trim.
        let err = resolve_command("run ", &cfg).unwrap_err();
        assert!(err.contains("not allowed"), "empty run inner should be rejected: {err}");
    }

    #[test]
    fn test_resolve_run_exact_prefix_no_args() {
        let cfg = default_cfg();
        // Exact prefix match with no additional args is allowed.
        assert_eq!(resolve_command("run npm", &cfg).unwrap(), "npm");
        assert_eq!(resolve_command("run pnpm", &cfg).unwrap(), "pnpm");
    }

    #[test]
    fn test_resolve_run_cargo_subcommands() {
        let cfg = default_cfg();
        assert_eq!(resolve_command("run cargo build", &cfg).unwrap(), "cargo build");
        assert_eq!(resolve_command("run cargo test --release", &cfg).unwrap(), "cargo test --release");
        assert_eq!(
            resolve_command("run cargo clippy -- -D warnings", &cfg).unwrap(),
            "cargo clippy -- -D warnings"
        );
    }

    #[test]
    fn test_resolve_error_lists_allowed_prefixes() {
        let cfg = default_cfg();
        let err = resolve_command("run rm -rf /", &cfg).unwrap_err();
        assert!(
            err.contains("cargo") || err.contains("npm") || err.contains("pnpm"),
            "error should list allowed prefixes: {err}"
        );
    }

    #[test]
    fn test_resolve_error_lists_aliases() {
        let cfg = default_cfg();
        let err = resolve_command("nonexistent", &cfg).unwrap_err();
        // Should mention some known alias.
        assert!(
            err.contains("test") || err.contains("build") || err.contains("check"),
            "error should list aliases: {err}"
        );
    }

    #[test]
    fn test_exec_config_default_aliases() {
        let cfg = ExecConfig::default();
        assert!(cfg.commands.contains_key("test"));
        assert!(cfg.commands.contains_key("build"));
        assert!(cfg.commands.contains_key("check"));
    }

    #[test]
    fn test_exec_config_default_run_prefixes() {
        let cfg = ExecConfig::default();
        assert!(cfg.run_prefixes.contains(&"cargo".to_string()));
        assert!(cfg.run_prefixes.contains(&"npm".to_string()));
        assert!(cfg.run_prefixes.contains(&"pnpm".to_string()));
    }

    #[test]
    fn test_resolve_no_run_prefixes_rejects_all_run() {
        let cfg = ExecConfig {
            commands: std::collections::HashMap::new(),
            run_prefixes: vec![],
        };
        let err = resolve_command("run cargo test", &cfg).unwrap_err();
        assert!(err.contains("not allowed"), "no prefixes → rejected: {err}");
    }

    #[test]
    fn test_resolve_custom_run_prefix() {
        let cfg = ExecConfig {
            commands: std::collections::HashMap::new(),
            run_prefixes: vec!["python".to_string()],
        };
        assert_eq!(resolve_command("run python main.py", &cfg).unwrap(), "python main.py");
        let err = resolve_command("run cargo build", &cfg).unwrap_err();
        assert!(err.contains("not allowed"), "non-configured prefix rejected: {err}");
    }
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
