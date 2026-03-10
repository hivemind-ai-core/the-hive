//! Hive Server binary — coordination control plane for The Hive.

use hive_server::{db, state, ws};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let filter = EnvFilter::new(format!("warn,hive_server={level},hive_core={level}"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("Hive Server starting...");

    let port: u16 = std::env::var("HIVE_SERVER_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()?;

    let db_path = std::env::var("HIVE_DB_PATH").unwrap_or_else(|_| "/data/hive.db".to_string());

    info!("  Port: {}", port);
    info!("  Database: {}", db_path);

    let pool = db::open(&db_path)?;
    db::run_migrations(&pool)?;
    info!("Database ready");

    let state = state::AppState::new(pool);
    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    ws::serve(listener, state).await?;

    Ok(())
}
