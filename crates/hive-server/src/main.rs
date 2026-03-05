//! Hive Server - Coordination control plane for The Hive.
//!
//! Runs in a Docker container and handles:
//! - Task tracker (create, list, claim, update, complete)
//! - Message board (topics, comments)
//! - Push message routing
//! - Agent registry

mod communication;
mod db;
mod handlers;
mod message_board;
mod state;
mod tasks;
mod ws;

use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Hive Server starting...");

    let port: u16 = std::env::var("HIVE_SERVER_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()?;

    let db_path = std::env::var("HIVE_DB_PATH")
        .unwrap_or_else(|_| "/data/hive.db".to_string());

    info!("  Port: {}", port);
    info!("  Database: {}", db_path);

    let pool = db::open(&db_path)?;
    db::run_migrations(&pool)?;
    info!("Database ready");

    let state = state::AppState::new(pool);
    let addr = format!("0.0.0.0:{port}").parse()?;

    ws::serve(addr, state).await?;

    Ok(())
}
