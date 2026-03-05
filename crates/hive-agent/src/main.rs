//! Hive Agent - Agent executor for The Hive.
//!
//! Runs in a Docker container and:
//! - Executes coding agents (Kilo, Claude Code) as subprocesses
//! - Runs an MCP server exposing coordination tools
//! - Manages session resumption for continuity within tasks

mod agent;
mod client;
mod executor;
mod mcp;
mod polling;
mod session;

use tracing::info;

use agent::Agent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Hive Agent starting...");

    let agent_id = std::env::var("HIVE_AGENT_ID").expect("HIVE_AGENT_ID is required");
    let server_url = std::env::var("HIVE_SERVER_URL").expect("HIVE_SERVER_URL is required");
    let app_daemon_url = std::env::var("HIVE_APP_DAEMON_URL").expect("HIVE_APP_DAEMON_URL is required");
    let coding_agent = std::env::var("CODING_AGENT").unwrap_or_else(|_| "kilo".to_string());
    let agent_name = std::env::var("HIVE_AGENT_NAME").unwrap_or_else(|_| agent_id.clone());
    let agent_tags: Vec<String> = std::env::var("HIVE_AGENT_TAGS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

    info!("  Agent ID: {}", agent_id);
    info!("  Server URL: {}", server_url);
    info!("  App Daemon URL: {}", app_daemon_url);
    info!("  Coding Agent: {}", coding_agent);

    let (cmd_tx, pending, _push_rx) = client::start(server_url, agent_id.clone(), agent_name, agent_tags.clone());

    start_mcp_server(&agent_id, &app_daemon_url, cmd_tx.clone(), pending.clone());

    let agent = Agent::new(agent_id, agent_tags, coding_agent, cmd_tx.clone(), pending);
    agent.spawn_polling();

    tokio::signal::ctrl_c().await?;
    let _ = cmd_tx.send(client::ClientCmd::Shutdown);
    info!("Shutting down");

    Ok(())
}

fn start_mcp_server(agent_id: &str, app_daemon_url: &str, cmd_tx: tokio::sync::mpsc::UnboundedSender<client::ClientCmd>, pending: client::PendingRequests) {
    let mcp_port: u16 = std::env::var("HIVE_MCP_PORT")
        .unwrap_or_else(|_| "7890".to_string())
        .parse()
        .unwrap_or(7890);

    let mcp_state = mcp::server::McpState {
        agent_id: agent_id.to_string(),
        cmd_tx,
        pending,
        app_daemon_url: app_daemon_url.to_string(),
        http: reqwest::Client::new(),
    };
    tokio::spawn(async move {
        if let Err(e) = mcp::server::serve(mcp_port, mcp_state).await {
            tracing::error!("MCP server error: {e}");
        }
    });
}
