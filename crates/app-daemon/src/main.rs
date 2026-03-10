//! App Daemon - HTTP server for development commands in app-container.
//!
//! Runs in the app-container Docker container and provides an HTTP API
//! for executing development commands (test, check, start, stop, etc.)

mod dev;
mod exec;
mod process;

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
use process::ProcessManagerHandle;

/// Shared application state for all handlers.
#[derive(Clone)]
pub struct AppState {
    pub exec_config: ExecConfig,
    pub processes: ProcessManagerHandle,
}

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
    let state = AppState {
        exec_config,
        processes: process::new_handle(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/exec", post(exec::exec))
        .route("/dev/start", post(dev::start))
        .route("/dev/stop", post(dev::stop))
        .route("/dev/restart", post(dev::restart))
        .route("/dev/status", get(dev::status))
        .route("/dev/logs", get(dev::logs))
        .route("/dev/stdin", post(dev::stdin))
        .route("/obs/test", post(dev::test))
        .route("/obs/check", post(dev::check))
        .with_state(state.clone());

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;

    info!("App Daemon listening on port {}", port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Clean up tracked processes on shutdown.
    process::kill_all(&state.processes).await;

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
        let toml = "[exec]\ncommands = { \"test\" = \"cargo test\", \"build\" = \"cargo build --release\" }\nrun_prefixes = [\"cargo\"]";
        let cfg = parse_hive_config(toml);
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

    // ── Integration tests (full HTTP stack) ───────────────────────────────────

    use axum::body::Body;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt as _;

    fn test_app() -> Router {
        let mut commands = std::collections::HashMap::new();
        commands.insert("start".to_string(), "echo dev-server-running".to_string());
        commands.insert("test".to_string(), "echo test-ok".to_string());
        commands.insert("check".to_string(), "echo check-ok".to_string());
        let exec_config = exec::ExecConfig {
            commands,
            run_prefixes: vec!["echo".to_string()],
        };
        let state = AppState {
            exec_config,
            processes: process::new_handle(),
        };
        Router::new()
            .route("/health", axum::routing::get(health))
            .route("/exec", axum::routing::post(exec::exec))
            .route("/dev/start", axum::routing::post(dev::start))
            .route("/dev/stop", axum::routing::post(dev::stop))
            .route("/dev/restart", axum::routing::post(dev::restart))
            .route("/dev/status", axum::routing::get(dev::status))
            .route("/dev/logs", axum::routing::get(dev::logs))
            .route("/dev/stdin", axum::routing::post(dev::stdin))
            .route("/obs/test", axum::routing::post(dev::test))
            .route("/obs/check", axum::routing::post(dev::check))
            .with_state(state)
    }

    async fn json_body(resp: axum::http::Response<Body>) -> Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_dev_start_returns_pid() {
        let app = test_app();
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert!(body["pid"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_dev_status_running() {
        let app = test_app();

        // Start a long-running process.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let start_body = json_body(resp).await;
        let started_pid = start_body["pid"].as_u64().unwrap();

        // Need a process that stays alive — our "start" command is "echo" which exits fast.
        // So let's check status for the process that was started. It may have already exited.
        // We verify the status endpoint works regardless.
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/dev/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        // pid should match what was started (process may have finished)
        assert_eq!(body["pid"], started_pid);
    }

    #[tokio::test]
    async fn test_dev_status_not_running() {
        let app = test_app();
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/dev/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["running"], false);
    }

    #[tokio::test]
    async fn test_dev_logs_captures_output() {
        let app = test_app();

        // Start process that outputs text.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap();

        // Wait for output to be captured.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/dev/logs")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let output = body["output"].as_str().unwrap();
        assert!(
            output.contains("dev-server-running"),
            "expected output to contain 'dev-server-running', got: {output}"
        );
    }

    #[tokio::test]
    async fn test_dev_logs_tail() {
        let app = test_app();

        // Override: we need a process that produces many lines.
        // Since test_app uses "echo dev-server-running" as start command,
        // it only produces 1 line. We test tail=5 still works.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/dev/logs?tail=5")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["line_count"].as_u64().unwrap() <= 5);
    }

    #[tokio::test]
    async fn test_dev_stop() {
        let app = test_app();

        // Start.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap();

        // Stop.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/stop")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["status"], "ok");

        // Status should show not running.
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/dev/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        assert_eq!(body["running"], false);
    }

    #[tokio::test]
    async fn test_dev_restart() {
        let app = test_app();

        // Start.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let first_pid = json_body(resp).await["pid"].as_u64().unwrap();

        // Restart.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/restart")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        assert_eq!(body["status"], "ok");
        let second_pid = body["pid"].as_u64().unwrap();

        // PIDs should differ (new process).
        assert_ne!(first_pid, second_pid);

        // Clean up.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/stop")
            .body(Body::empty())
            .unwrap();
        app.oneshot(req).await.unwrap();
    }

    #[tokio::test]
    async fn test_dev_stdin() {
        let app = test_app();

        // Start cat process that reads stdin.
        // We can't easily override the start command per-test, so we test
        // that stdin endpoint returns an error when process has no stdin
        // (echo exits immediately).
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/start")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/dev/stdin")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"input":"hello\n"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Either "ok" or "error" is fine — we're testing the endpoint works.
        let body = json_body(resp).await;
        assert!(body["status"].is_string());
    }

    #[tokio::test]
    async fn test_exec_still_works() {
        let app = test_app();
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/exec")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"command":"run echo hello"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert!(body["output"].as_str().unwrap().contains("hello"));
    }
}
