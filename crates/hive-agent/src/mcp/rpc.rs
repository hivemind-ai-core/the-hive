//! RPC helper: send a request to hive-server and await the response.
//!
//! Uses a shared `PendingRequests` map (id → oneshot sender) that is
//! populated before sending and resolved when the response arrives.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use hive_core::types::ApiMessage;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::client::ClientCmd;

pub type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<ApiMessage>>>>;

/// Send `request` to the server and block until the matching response arrives.
pub async fn call_server(
    cmd_tx: &UnboundedSender<ClientCmd>,
    request: ApiMessage,
) -> Result<serde_json::Value> {
    // This simpler version just sends the request without waiting for a
    // response — the MCP tools use fire-and-forget for now, returning the
    // request id so the caller can correlate later if needed.
    //
    // A full request/response bridge would require the polling loop to route
    // responses back to pending oneshots (added in a future task).
    let id = request.id.clone();
    cmd_tx
        .send(ClientCmd::Send(request))
        .map_err(|_| anyhow!("WS client channel closed"))?;

    // Short wait to let the message propagate (best-effort).
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(serde_json::json!({ "request_id": id, "status": "sent" }))
}
