//! MCP push tool implementations (added in task 42).

use anyhow::Result;
use serde_json::Value;

use crate::client;
use crate::mcp::rpc::call_server;
use crate::mcp::server::McpState;

pub async fn send(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("push.send", params);
    call_server(state, req).await
}

pub async fn list(state: &McpState, _params: Option<Value>) -> Result<Value> {
    let req = client::request(
        "push.list",
        Some(serde_json::json!({ "agent_id": state.agent_id })),
    );
    call_server(state, req).await
}
