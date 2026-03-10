//! App Daemon - HTTP server for development commands in app-container.
//!
//! Runs in the app-container Docker container and provides an HTTP API
//! for executing development commands (test, check, start, stop, etc.)

mod dev;
mod exec;

use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use exec::ExecConfig;

/// Minimal wrapper to deserialize only the `[exec]` table from config.toml.
#[derive(Debug, Deserialize, Default)]
struct HiveConfig {
    #[serde(default)]
    exec: ExecConfig,
}

fn load_exec_config() -> ExecConfig {
    let config_path =
        std::env::var("HIVE_CONFIG_PATH").unwrap_or_else(|_| "/app/.hive/config.toml".to_string());

    match std::fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str::<HiveConfig>(&content) {
            Ok(cfg) => {
                info!("Loaded exec config from {}", config_path);
                cfg.exec
            }
            Err(e) => {
                warn!("Failed to parse {}: {e}; using defaults", config_path);
                ExecConfig::default()
            }
        },
        Err(_) => {
            info!("No config file at {}; using exec defaults", config_path);
            ExecConfig::default()
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let filter = EnvFilter::new(format!("warn,app_daemon={level},hive_core={level}"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("App Daemon starting...");

    let port = std::env::var("HIVE_APP_DAEMON_PORT").unwrap_or_else(|_| "8081".to_string());

    info!("  Port: {}", port);

    let exec_config = load_exec_config();

    let app = Router::new()
        .route("/health", get(health))
        .route("/exec", post(exec::exec))
        .route("/dev/start", post(dev::start))
        .route("/dev/stop", post(dev::stop))
        .route("/dev/restart", post(dev::restart))
        .route("/obs/test", post(dev::test))
        .route("/obs/check", post(dev::check))
        .route("/obs/logs", post(dev::logs))
        .with_state(exec_config);

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;

    info!("App Daemon listening on port {}", port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    info!("Shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn parse_hive_config(toml: &str) -> ExecConfig {
        toml::from_str::<HiveConfig>(toml).unwrap_or_default().exec
    }

    fn load_from_file(content: &str) -> ExecConfig {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        // Override env var so load_exec_config reads our temp file.
        std::env::set_var("HIVE_CONFIG_PATH", &path);
        let cfg = load_exec_config();
        std::env::remove_var("HIVE_CONFIG_PATH");
        cfg
    }

    #[test]
    fn test_missing_config_file_uses_defaults() {
        std::env::set_var(
            "HIVE_CONFIG_PATH",
            "/tmp/hive-nonexistent-config-99999.toml",
        );
        let cfg = load_exec_config();
        std::env::remove_var("HIVE_CONFIG_PATH");

        // Default aliases must be present.
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("pnpm test")
        );
        assert_eq!(
            cfg.commands.get("build").map(String::as_str),
            Some("pnpm build")
        );
    }

    #[test]
    fn test_valid_config_overrides_commands() {
        let toml = r#"
[exec]
commands = { "test" = "cargo test", "build" = "cargo build --release" }
run_prefixes = ["cargo"]
"#;
        let cfg = load_from_file(toml);
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("cargo test")
        );
        assert_eq!(
            cfg.commands.get("build").map(String::as_str),
            Some("cargo build --release")
        );
        assert_eq!(cfg.run_prefixes, vec!["cargo"]);
    }

    #[test]
    fn test_partial_config_missing_commands_uses_struct_default() {
        // #[serde(default)] at struct level calls ExecConfig::default() for the whole struct,
        // then overrides only the fields present in the TOML. Missing fields get the struct's
        // Default values, NOT the type-level defaults (e.g. HashMap::new()).
        // So specifying only run_prefixes still preserves the default command aliases.
        let cfg = parse_hive_config("[exec]\nrun_prefixes = [\"cargo\"]");
        assert!(
            !cfg.commands.is_empty(),
            "Struct-level #[serde(default)] preserves default command aliases for missing fields"
        );
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("pnpm test")
        );
        assert_eq!(cfg.run_prefixes, vec!["cargo"]);
    }

    #[test]
    fn test_malformed_toml_falls_back_to_defaults() {
        let cfg = load_from_file("this is not [ valid toml {{{{");
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("pnpm test"),
            "malformed TOML should fall back to defaults"
        );
    }

    #[test]
    fn test_empty_exec_section_uses_struct_default() {
        // [exec] present but empty → struct-level #[serde(default)] kicks in for all fields.
        // ExecConfig::default() is used, so all aliases and run_prefixes are present.
        let cfg = parse_hive_config("[exec]");
        assert!(
            !cfg.commands.is_empty(),
            "empty [exec] should still have default aliases"
        );
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("pnpm test")
        );
        assert!(!cfg.run_prefixes.is_empty());
    }

    #[test]
    fn test_no_exec_section_uses_struct_default() {
        // No [exec] section at all → HiveConfig uses #[serde(default)] → ExecConfig::default().
        let cfg = parse_hive_config("[other_section]\nfoo = 1");
        assert_eq!(
            cfg.commands.get("test").map(String::as_str),
            Some("pnpm test"),
            "missing [exec] section should use ExecConfig::default()"
        );
    }
}
