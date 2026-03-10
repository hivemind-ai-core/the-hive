//! `app.dev` MCP tool: dev server lifecycle management via the app-daemon.

use anyhow::{bail, Result};
use serde_json::Value;

use crate::mcp::server::McpState;

pub async fn dev(state: &McpState, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let action = p
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.action is required"))?;

    let base = &state.app_daemon_url;

    match action {
        "start" => {
            let resp: Value = state.http.post(format!("{base}/dev/start")).send().await?.json().await?;
            Ok(resp)
        }
        "stop" => {
            let resp: Value = state.http.post(format!("{base}/dev/stop")).send().await?.json().await?;
            Ok(resp)
        }
        "restart" => {
            let resp: Value = state.http.post(format!("{base}/dev/restart")).send().await?.json().await?;
            Ok(resp)
        }
        "status" => {
            let resp: Value = state.http.get(format!("{base}/dev/status")).send().await?.json().await?;
            Ok(resp)
        }
        "logs" => {
            let tail = p.get("tail").and_then(|v| v.as_u64());
            let mut url = format!("{base}/dev/logs");
            if let Some(n) = tail {
                url = format!("{url}?tail={n}");
            }
            let resp: Value = state.http.get(&url).send().await?.json().await?;
            Ok(resp)
        }
        "stdin" => {
            let input = p
                .get("input")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("params.input is required for stdin action"))?;
            let body = serde_json::json!({ "input": input });
            let resp: Value = state.http.post(format!("{base}/dev/stdin")).json(&body).send().await?.json().await?;
            Ok(resp)
        }
        other => bail!("unknown action '{other}'; expected: start, stop, restart, status, logs, stdin"),
    }
}
