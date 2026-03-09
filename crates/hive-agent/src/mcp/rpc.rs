//! RPC helper: send a request to hive-server and await the response.

use anyhow::Result;
use hive_core::types::ApiMessage;

use crate::mcp::server::McpState;

/// Send `request` to the server and block until the matching response arrives.
pub async fn call_server(state: &McpState, request: ApiMessage) -> Result<serde_json::Value> {
    match crate::client::send_request(&state.cmd_tx, &state.pending, request).await {
        Some(response) => {
            if let Some(err) = response.error {
                anyhow::bail!("{}", err.message);
            }
            Ok(response.result.unwrap_or(serde_json::Value::Null))
        }
        None => anyhow::bail!("no response from server (timeout or channel closed)"),
    }
}
