//! Spawn and manage a coding agent subprocess (kilo, claude, etc.).

use anyhow::{Context, Result};
use hive_core::types::{PushMessage, Task};
use tokio::process::Command;
use tracing::{info, warn};

pub struct ExecutionResult {
    pub exit_code: i32,
    pub output: String,
}

/// Build the prompt passed to the coding agent, prepending any push messages.
pub fn build_prompt(task: &Task, messages: &[PushMessage]) -> String {
    let mut prompt = String::new();

    if !messages.is_empty() {
        prompt.push_str("# Messages from other agents\n\n");
        for msg in messages {
            let from = msg.from_agent_id.as_deref().unwrap_or("server");
            prompt.push_str(&format!("[{}]: {}\n", from, msg.content));
        }
        prompt.push('\n');
    }

    prompt.push_str(&format!("# Task: {}\n\n", task.title));
    if let Some(desc) = &task.description {
        prompt.push_str(desc);
        prompt.push('\n');
    }
    if !task.tags.is_empty() {
        prompt.push_str(&format!("\nTags: {}\n", task.tags.join(", ")));
    }
    prompt
}

/// Write the MCP server config files so the coding agent subprocess can discover the hive tools.
///
/// Writes `.mcp.json` for Claude Code and `.kilocode/mcp.json` for Kilo, both pointing at
/// the hive TCP MCP server on 127.0.0.1:mcp_port.
fn write_mcp_configs(mcp_port: u16) {
    let config = serde_json::json!({
        "mcpServers": {
            "hive": {
                "transport": "tcp",
                "host": "127.0.0.1",
                "port": mcp_port
            }
        }
    });
    let content = serde_json::to_string_pretty(&config).unwrap_or_default();

    // Claude Code: .mcp.json in project root
    if let Err(e) = std::fs::write(".mcp.json", &content) {
        warn!("Failed to write .mcp.json: {e}");
    }

    // Kilo: .kilocode/mcp.json — merge the hive entry into any existing config.
    let kilocode_dir = std::path::Path::new(".kilocode");
    if let Err(e) = std::fs::create_dir_all(kilocode_dir) {
        warn!("Failed to create .kilocode/: {e}");
        return;
    }
    let kilo_path = kilocode_dir.join("mcp.json");
    let mut kilo_cfg: serde_json::Value = kilo_path
        .exists()
        .then(|| std::fs::read_to_string(&kilo_path).ok())
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "mcpServers": {} }));

    if let Some(servers) = kilo_cfg.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.insert("hive".to_string(), serde_json::json!({
            "transport": "tcp",
            "host": "127.0.0.1",
            "port": mcp_port
        }));
    }
    let kilo_content = serde_json::to_string_pretty(&kilo_cfg).unwrap_or_default();
    if let Err(e) = std::fs::write(&kilo_path, kilo_content) {
        warn!("Failed to write .kilocode/mcp.json: {e}");
    }
}

/// Execute the coding agent with the given task and any pending push messages.
///
/// `agent_bin` is the agent executable name (`kilo`, `claude`, etc.).
/// `agent_id` is used to load/save session state for resumption.
pub async fn run(
    task: &Task,
    agent_bin: &str,
    agent_id: &str,
    messages: &[PushMessage],
) -> Result<ExecutionResult> {
    let prompt = build_prompt(task, messages);
    info!("Spawning '{agent_bin}' for task: {}", task.id);

    let mcp_port: u16 = std::env::var("HIVE_MCP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7890);
    write_mcp_configs(mcp_port);

    let session_id = crate::session::load(agent_id);

    let mut cmd = Command::new(agent_bin);
    cmd.env("TASK_ID", &task.id)
        .env("TASK_TITLE", &task.title);

    match agent_bin {
        "claude" => {
            // claude --dangerously-skip-permissions [-r <session_id>] -p <prompt>
            cmd.arg("--dangerously-skip-permissions");
            if let Some(ref sid) = session_id {
                cmd.args(["-r", sid]);
            }
            cmd.arg("-p").arg(&prompt);
        }
        "kilo" => {
            // kilo run --auto [-c -s <session_id>] <prompt>
            cmd.args(["run", "--auto"]);
            if let Some(ref sid) = session_id {
                cmd.args(["-c", "-s", sid]);
            }
            cmd.arg(&prompt);
        }
        other => {
            // Unknown agent: pass prompt as sole argument.
            warn!("Unknown coding agent '{other}', passing prompt directly");
            cmd.arg(&prompt);
        }
    }

    let output = cmd
        .output()
        .await
        .with_context(|| format!("spawning '{agent_bin}'"))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.is_empty() {
        stdout.into_owned()
    } else {
        format!("{stdout}{stderr}")
    };

    info!("'{agent_bin}' finished with exit code {exit_code}");

    // Persist session ID for next run.
    if let Some(session_id) = crate::session::extract_from_output(&combined) {
        if let Err(e) = crate::session::save(agent_id, &session_id) {
            warn!("Failed to save session for agent '{agent_id}': {e}");
        }
    }

    Ok(ExecutionResult {
        exit_code,
        output: combined,
    })
}
