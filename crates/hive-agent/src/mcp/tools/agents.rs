//! MCP agent tool implementations.

use anyhow::Result;
use serde_json::Value;

use crate::client;
use crate::mcp::rpc::call_server;
use crate::mcp::server::McpState;

pub async fn list(state: &McpState, _params: Option<Value>) -> Result<Value> {
    let req = client::request("agent.list", None);
    call_server(state, req).await
}
