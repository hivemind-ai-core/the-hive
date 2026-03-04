//! Hive Agent - Agent executor for The Hive.
//!
//! Runs in a Docker container and:
//! - Executes coding agents (Kilo, Claude Code) as subprocesses
//! - Runs an MCP server exposing coordination tools
//! - Manages session resumption for continuity within tasks

use tracing::info;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Hive Agent starting...");

    // Load configuration from environment
    let agent_id = std::env::var("HIVE_AGENT_ID")
        .expect("HIVE_AGENT_ID is required");
    
    let server_url = std::env::var("HIVE_SERVER_URL")
        .expect("HIVE_SERVER_URL is required");
    
    let app_daemon_url = std::env::var("HIVE_APP_DAEMON_URL")
        .expect("HIVE_APP_DAEMON_URL is required");

    let coding_agent = std::env::var("CODING_AGENT")
        .unwrap_or_else(|_| "kilo".to_string());

    info!("Agent configuration:");
    info!("  Agent ID: {}", agent_id);
    info!("  Server URL: {}", server_url);
    info!("  App Daemon URL: {}", app_daemon_url);
    info!("  Coding Agent: {}", coding_agent);

    // TODO: Connect to hive-server
    // TODO: Register agent
    // TODO: Start main agent loop
    // TODO: Start MCP server
    
    println!("Hive Agent - TODO: Implement agent");
    println!("Agent ID: {}", agent_id);
}
