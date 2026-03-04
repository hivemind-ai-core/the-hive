//! Hive Server - Coordination control plane for The Hive.
//!
//! Runs in a Docker container and handles:
//! - Task tracker (create, list, claim, update, complete)
//! - Message board (topics, comments)
//! - Push message routing
//! - Agent registry

use tracing::info;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Hive Server starting...");
    
    let port = std::env::var("HIVE_SERVER_PORT")
        .unwrap_or_else(|_| "8080".to_string());
    
    let db_path = std::env::var("HIVE_DB_PATH")
        .unwrap_or_else(|_| "/data/hive.db".to_string());

    info!("Server configuration:");
    info!("  Port: {}", port);
    info!("  Database: {}", db_path);

    // TODO: Initialize database
    // TODO: Start WebSocket server
    // TODO: Implement API handlers
    
    println!("Hive Server - TODO: Implement server");
    println!("Listening on port {}", port);
}
