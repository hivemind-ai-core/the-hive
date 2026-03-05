//! MCP task tool implementations.

use anyhow::Result;
use serde_json::Value;

use crate::client::{self, ClientCmd};
use crate::mcp::server::McpState;
use crate::mcp::rpc::call_server;

pub async fn get_next(state: &McpState, params: Option<Value>) -> Result<Value> {
    let tag = params.as_ref().and_then(|v| v.get("tag")).cloned();
    let req = client::request(
        "task.get_next",
        Some(serde_json::json!({
            "agent_id": state.agent_id,
            "tag": tag,
        })),
    );
    call_server(&state.cmd_tx, req).await
}

pub async fn complete(state: &McpState, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let req = client::request("task.complete", Some(p));
    call_server(&state.cmd_tx, req).await
}
