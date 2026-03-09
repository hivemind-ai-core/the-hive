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
use tracing_subscriber::EnvFilter;

use agent::Agent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let filter = EnvFilter::new(format!(
        "warn,hive_agent={level},hive_core={level}"
    ));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("Hive Agent starting...");

    apply_kilo_auth();
    apply_claude_auth();

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

    let (cmd_tx, pending, mut push_rx) = client::start(server_url, agent_id.clone(), agent_name, agent_tags.clone());

    // Heartbeat: update last_seen_at every 30 seconds.
    {
        let hb_tx = cmd_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let msg = client::request("agent.heartbeat", None);
                if hb_tx.send(client::ClientCmd::Send(msg)).is_err() {
                    break;
                }
                tracing::debug!("heartbeat sent");
            }
        });
    }

    start_mcp_server(&agent_id, &app_daemon_url, cmd_tx.clone(), pending.clone());

    let agent = Agent::new(agent_id, agent_tags, coding_agent, cmd_tx.clone(), pending);
    agent.spawn_polling();

    // Log incoming agent-to-agent push messages so they're visible in `hive logs`.
    // Server-wide broadcast updates (tasks.updated, agents.updated, topics.updated)
    // are suppressed at INFO to avoid log spam.
    tokio::spawn(async move {
        while let Some(msg) = push_rx.recv().await {
            let method = msg.method.as_deref().unwrap_or("");
            if matches!(method, "tasks.updated" | "agents.updated" | "topics.updated") {
                tracing::debug!("Server broadcast: {method}");
                continue;
            }
            if let Some(params) = msg.params {
                info!("Push message received [{method}]: {params}");
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    let _ = cmd_tx.send(client::ClientCmd::Shutdown);
    info!("Shutting down");

    Ok(())
}

/// If `KILO_PROVIDER_JSON` is set in the environment, write it to
/// `$HOME/.kilocode/cli/config.json` so the kilo CLI picks it up.
fn apply_kilo_auth() {
    let Ok(json_str) = std::env::var("KILO_PROVIDER_JSON") else { return };
    if json_str.is_empty() { return }
    let home = match std::env::var("HOME").ok().map(std::path::PathBuf::from) {
        Some(p) => p,
        None => { tracing::warn!("KILO_PROVIDER_JSON set but HOME is unset — skipping"); return }
    };
    let dst = home.join(".kilocode/cli/config.json");
    if let Some(parent) = dst.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Failed to create ~/.kilocode/cli: {e}");
            return;
        }
    }
    if let Err(e) = std::fs::write(&dst, &json_str) {
        tracing::warn!("Failed to write kilo provider config: {e}");
    } else {
        info!("Kilo provider config written to {}", dst.display());
    }
}

/// If `CLAUDE_AUTH_JSON` is set in the environment, write it to
/// `$HOME/.claude.json` so the claude CLI picks it up.
fn apply_claude_auth() {
    let Ok(json_str) = std::env::var("CLAUDE_AUTH_JSON") else { return };
    if json_str.is_empty() { return }
    let home = match std::env::var("HOME").ok().map(std::path::PathBuf::from) {
        Some(p) => p,
        None => { tracing::warn!("CLAUDE_AUTH_JSON set but HOME is unset — skipping"); return }
    };
    let dst = home.join(".claude.json");
    if let Err(e) = std::fs::write(&dst, &json_str) {
        tracing::warn!("Failed to write claude auth: {e}");
    } else {
        info!("Claude auth written to {}", dst.display());
    }
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
