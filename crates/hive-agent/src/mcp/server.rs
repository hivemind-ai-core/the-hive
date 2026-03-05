//! MCP JSON-RPC HTTP server.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

pub use reqwest::Client as HttpClient;

use crate::client::ClientCmd;

/// JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcResponse {
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn err(id: Option<Value>, code: i32, message: String) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(RpcError { code, message }) }
    }
}

#[derive(Clone)]
pub struct McpState {
    pub agent_id: String,
    pub cmd_tx: UnboundedSender<ClientCmd>,
    pub app_daemon_url: String,
    pub http: reqwest::Client,
}

/// Start the MCP HTTP server. Does not return (runs until process exits).
pub async fn serve(port: u16, state: McpState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", post(handle_rpc))
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("MCP server listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_rpc(
    State(state): State<McpState>,
    Json(req): Json<RpcRequest>,
) -> (StatusCode, Json<RpcResponse>) {
    if req.jsonrpc != "2.0" {
        return (
            StatusCode::BAD_REQUEST,
            Json(RpcResponse::err(req.id, -32600, "invalid JSON-RPC version".into())),
        );
    }

    let result = dispatch(&req.method, req.params, &state).await;

    match result {
        Ok(v) => (StatusCode::OK, Json(RpcResponse::ok(req.id, v))),
        Err(e) => (
            StatusCode::OK, // JSON-RPC errors still return 200
            Json(RpcResponse::err(req.id, -32603, e.to_string())),
        ),
    }
}

async fn dispatch(
    method: &str,
    params: Option<Value>,
    state: &McpState,
) -> anyhow::Result<Value> {
    use super::tools;
    match method {
        "task.get_next"  => tools::tasks::get_next(state, params).await,
        "task.complete"  => tools::tasks::complete(state, params).await,
        "topic.create"   => tools::topics::create(state, params).await,
        "topic.list"     => tools::topics::list(state, params).await,
        "topic.get"      => tools::topics::get(state, params).await,
        "topic.comment"  => tools::topics::comment(state, params).await,
        "topic.wait"     => tools::topics::wait(state, params).await,
        "push.send"      => tools::push::send(state, params).await,
        "push.list"      => tools::push::list(state, params).await,
        "app.exec"       => tools::app_exec::exec(state, params).await,
        _ => anyhow::bail!("method not found: {method}"),
    }
}
