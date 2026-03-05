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

    let mut cmd = Command::new(agent_bin);
    cmd.arg("--print") // non-interactive mode
        .env("TASK_ID", &task.id)
        .env("TASK_TITLE", &task.title);

    // Resume previous session if one exists.
    if let Some(session_id) = crate::session::load(agent_id) {
        for arg in crate::session::resume_args(agent_bin, &session_id) {
            cmd.arg(arg);
        }
    }

    cmd.arg(&prompt);

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
