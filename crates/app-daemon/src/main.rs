//! App Daemon - HTTP server for development commands in app-container.
//!
//! Runs in the app-container Docker container and provides an HTTP API
//! for executing development commands (test, check, start, stop, etc.)

mod dev;
mod exec;

use axum::{Router, routing::{get, post}, Json};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{info, warn};

use exec::ExecConfig;

/// Minimal wrapper to deserialize only the `[exec]` table from config.toml.
#[derive(Debug, Deserialize, Default)]
struct HiveConfig {
    #[serde(default)]
    exec: ExecConfig,
}

fn load_exec_config() -> ExecConfig {
    let config_path = std::env::var("HIVE_CONFIG_PATH")
        .unwrap_or_else(|_| "/config/config.toml".to_string());

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
    tracing_subscriber::fmt::init();

    info!("App Daemon starting...");

    let port = std::env::var("HIVE_APP_DAEMON_PORT")
        .unwrap_or_else(|_| "8081".to_string());

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

    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))
        .await?;

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
