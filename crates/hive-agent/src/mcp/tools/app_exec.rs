//! app_exec MCP tool: forward exec requests to the app-daemon.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp::server::McpState;

#[derive(Serialize)]
struct ExecRequest {
    command: String,
    pattern: Option<String>,
}

#[derive(Deserialize)]
struct ExecResponse {
    status: String,
    output: String,
    exit_code: i32,
}

pub async fn exec(state: &McpState, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let command = p
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.command is required"))?
        .to_string();
    let pattern = p.get("pattern").and_then(|v| v.as_str()).map(str::to_string);

    let app_daemon_url = &state.app_daemon_url;
    let url = format!("{app_daemon_url}/exec");

    let body = ExecRequest { command, pattern };
    let resp = state
        .http
        .post(&url)
        .json(&body)
        .send()
        .await?
        .json::<ExecResponse>()
        .await?;

    Ok(serde_json::json!({
        "status": resp.status,
        "output": resp.output,
        "exit_code": resp.exit_code,
    }))
}
