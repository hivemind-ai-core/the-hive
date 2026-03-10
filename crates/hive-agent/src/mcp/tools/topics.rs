//! MCP topic tool implementations (added in task 41).

use anyhow::Result;
use serde_json::Value;

use crate::client;
use crate::mcp::rpc::call_server;
use crate::mcp::server::McpState;

pub async fn create(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("topic.create", params);
    call_server(state, req).await
}

pub async fn list(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("topic.list", params);
    call_server(state, req).await
}

pub async fn get(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("topic.get", params);
    call_server(state, req).await
}

pub async fn comment(state: &McpState, params: Option<Value>) -> Result<Value> {
    let req = client::request("topic.comment", params);
    call_server(state, req).await
}

/// Blocking wait: poll topic.get until comment count increases, up to timeout.
pub async fn wait(state: &McpState, params: Option<Value>) -> Result<Value> {
    use std::time::Duration;
    let p = params.clone().unwrap_or(Value::Null);
    let timeout_secs = p.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30);
    let expected_min = p.get("min_comments").and_then(|v| v.as_u64()).unwrap_or(1);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let req = client::request("topic.get", params.clone());
        let result = call_server(state, req).await?;
        let count = result
            .get("comments")
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len() as u64);
        if count >= expected_min {
            return Ok(result);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for comments");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
