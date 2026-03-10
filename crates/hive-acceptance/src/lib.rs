//! Shared test helpers for hive acceptance tests.
//!
//! Each test file in `tests/` imports from this crate to get a running server,
//! WebSocket connections, and a consistent way to call API methods.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiMessage, MessageType};
use hive_server::{db, state, ws};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

pub use serde_json::{json, Value};

// ── Server lifecycle ──────────────────────────────────────────────────────────

/// Start a hive-server on a random port with an in-memory database.
///
/// Returns the local address the server is listening on.
pub async fn start_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let pool = open_test_db();
        let state = state::AppState::new(pool);
        ws::serve(listener, state).await.unwrap();
    });

    // Give the server a moment to accept connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

fn open_test_db() -> db::DbPool {
    let pool = db::open(":memory:").expect("open in-memory db");
    db::run_migrations(&pool).expect("run migrations");
    pool
}

// ── WebSocket client ──────────────────────────────────────────────────────────

/// A connected WebSocket client using the concrete tungstenite stream type.
pub type WsClient = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect a WebSocket client to the server as the given agent.
pub async fn connect(addr: SocketAddr, agent_id: &str) -> WsClient {
    let url = format!("ws://{addr}/ws?agent_id={agent_id}");
    let (ws, _) = connect_async(&url).await.unwrap();
    ws
}

// ── Request / response helpers ────────────────────────────────────────────────

/// A parsed API response.
pub struct Response {
    pub result: Option<Value>,
    pub error: Option<Value>,
}

/// Send a JSON request and await the response.
pub async fn call(ws: &mut WsClient, method: &str, params: Value) -> Response {
    let id = Uuid::new_v4().to_string();
    let req = ApiMessage {
        msg_type: MessageType::Request,
        id: id.clone(),
        method: Some(method.to_string()),
        params: Some(params),
        result: None,
        error: None,
    };
    let text = serde_json::to_string(&req).unwrap();
    ws.send(Message::Text(text.into())).await.unwrap();

    loop {
        let msg = ws.next().await.unwrap().unwrap();
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["id"].as_str() == Some(&id) {
                let non_null = |val: Option<&Value>| -> Option<Value> {
                    val.and_then(|v| if v.is_null() { None } else { Some(v.clone()) })
                };
                return Response {
                    result: non_null(v.get("result")),
                    error: non_null(v.get("error")),
                };
            }
        }
    }
}

// ── Push / notification helpers ─────────────────────────────────────────────

/// Read the next push message from the WebSocket (non-response, server-initiated).
///
/// Returns the full parsed JSON value of the push message.
/// Times out after `timeout` to avoid hanging tests.
pub async fn recv_push(ws: &mut WsClient, timeout: std::time::Duration) -> Option<Value> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            frame = ws.next() => {
                match frame {
                    Some(Ok(Message::Text(t))) => {
                        let v: Value = serde_json::from_str(&t).unwrap();
                        // Push messages have type "push" (not "response")
                        if v["type"].as_str() == Some("push") {
                            return Some(v);
                        }
                        // Skip response messages (they belong to `call`)
                    }
                    _ => return None,
                }
            }
        }
    }
}

/// Read the next push message matching a specific method.
///
/// Skips push messages that don't match `method`. Times out after `timeout`.
pub async fn recv_push_method(ws: &mut WsClient, method: &str, timeout: std::time::Duration) -> Option<Value> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            frame = ws.next() => {
                match frame {
                    Some(Ok(Message::Text(t))) => {
                        let v: Value = serde_json::from_str(&t).unwrap();
                        if v["type"].as_str() == Some("push") && v["method"].as_str() == Some(method) {
                            return Some(v);
                        }
                    }
                    _ => return None,
                }
            }
        }
    }
}
