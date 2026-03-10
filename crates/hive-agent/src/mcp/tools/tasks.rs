//! MCP task tool implementations.

use anyhow::Result;
use serde_json::Value;

use crate::client;
use crate::mcp::rpc::call_server;
use crate::mcp::server::McpState;

pub async fn get_next(state: &McpState, params: Option<Value>) -> Result<Value> {
    let tag = params.as_ref().and_then(|v| v.get("tag")).cloned();
    let req = client::request(
        "task.get_next",
        Some(serde_json::json!({
            "agent_id": state.agent_id,
            "tag": tag,
        })),
    );
    call_server(state, req).await
}

pub async fn complete(state: &McpState, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let req = client::request("task.complete", Some(p));
    call_server(state, req).await
}

pub async fn create(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.create", params);
    call_server(state, req).await
}

pub async fn list(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.list", params);
    call_server(state, req).await
}

pub async fn get(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.get", params);
    call_server(state, req).await
}

pub async fn update(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.update", params);
    call_server(state, req).await
}

pub async fn split(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.split", params);
    call_server(state, req).await
}

pub async fn set_dependency(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("task.set_dependency", params);
    call_server(state, req).await
}
