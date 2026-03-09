//! Spawn and manage a coding agent subprocess (kilo, claude, etc.).

use anyhow::{Context, Result};
use hive_core::types::{PushMessage, Task};
use tokio::process::Command;
use tracing::{debug, info, trace, warn};

pub struct ExecutionResult {
    pub exit_code: i32,
    pub output: String,
}

/// Build the prompt passed to the coding agent, prepending any push messages.
pub fn build_prompt(task: &Task, agent_id: &str, messages: &[PushMessage]) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!("# Your Identity\n\nYou are agent `{agent_id}`.\n\n"));

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
/// Claude Code and Kilo support Streamable HTTP MCP transport via a `url` entry.
/// We point them directly at the hive-agent's HTTP MCP server — no bridge needed.
fn write_mcp_configs(mcp_port: u16) {
    let url_entry = serde_json::json!({
        "url": format!("http://127.0.0.1:{mcp_port}/mcp")
    });

    // Claude Code: .mcp.json in project root
    let claude_cfg = serde_json::json!({ "mcpServers": { "hive": url_entry } });
    let content = serde_json::to_string_pretty(&claude_cfg).unwrap_or_default();
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
        servers.insert("hive".to_string(), url_entry);
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
    let prompt = build_prompt(task, agent_id, messages);
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

    // Log the command being run (no prompt text at INFO to avoid log flooding).
    let cmd_args: Vec<&str> = match agent_bin {
        "claude" => {
            let mut args = vec!["--dangerously-skip-permissions"];
            if session_id.is_some() { args.extend(["-r", "<session>"]); }
            args.push("-p"); args.push("<prompt>");
            args
        }
        "kilo" => {
            let mut args = vec!["run", "--auto"];
            if session_id.is_some() { args.extend(["-c", "-s", "<session>"]); }
            args.push("<prompt>");
            args
        }
        _ => vec!["<prompt>"],
    };
    info!("Running: {agent_bin} {}", cmd_args.join(" "));
    let preview = &prompt[..prompt.len().min(200)];
    debug!("Prompt preview: {preview}");
    trace!("Full prompt: {prompt}");

    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);
    cmd.kill_on_drop(true);

    let (exit_code, combined) = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            let combined = if stderr.is_empty() { stdout.clone() } else { format!("{stdout}{stderr}") };
            if exit_code == 0 {
                info!("'{agent_bin}' finished successfully");
            } else {
                warn!("'{agent_bin}' finished with exit code {exit_code}");
                if !stderr.is_empty() { warn!("stderr: {}", stderr.trim()); }
                if !stdout.is_empty() { warn!("stdout: {}", stdout.trim()); }
            }
            (exit_code, combined)
        }
        Ok(Err(e)) => {
            warn!("Failed to spawn '{agent_bin}': {e}");
            return Err(e).with_context(|| format!("spawning '{agent_bin}'"));
        }
        Err(_) => {
            warn!("'{agent_bin}' timed out after 10m for task {} — killing", task.id);
            (-1, "timed out after 10m".to_string())
        }
    };

    // Persist session ID for next run.
    match crate::session::extract_from_output(&combined) {
        Some(session_id) => {
            debug!("Extracted session id: {session_id}");
            if let Err(e) = crate::session::save(agent_id, &session_id) {
                warn!("Failed to save session for agent '{agent_id}': {e}");
            }
        }
        None => debug!("No session id found in output (context will not be resumed)"),
    }

    Ok(ExecutionResult {
        exit_code,
        output: combined,
    })
}
