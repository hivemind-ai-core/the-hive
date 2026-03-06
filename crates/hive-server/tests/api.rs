//! Integration tests for hive-server WebSocket API.
//!
//! Each test spins up a real server on a random port with an in-memory SQLite DB,
//! connects a WS client, exercises the API, and asserts the response.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiMessage, MessageType};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Start a hive-server on a random port, return its address.
async fn start_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Use an in-memory database so tests are isolated.
    // NOTE: hive-server internals are accessed via pub(crate); we use the library API.
    tokio::spawn(async move {
        let pool = hive_server_test_helpers::open_test_db();
        let state = hive_server_test_helpers::make_state(pool);
        hive_server_test_helpers::serve(listener, state).await;
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

/// Connect a WS client as a named agent.
async fn connect(addr: SocketAddr, agent_id: &str) -> impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin {
    let url = format!("ws://{addr}/ws?agent_id={agent_id}");
    let (ws, _) = connect_async(&url).await.unwrap();
    ws
}

/// Build a JSON-RPC request message.
fn req(method: &str, params: serde_json::Value) -> String {
    let msg = ApiMessage {
        msg_type: MessageType::Request,
        id: Uuid::new_v4().to_string(),
        method: Some(method.to_string()),
        params: Some(params),
        result: None,
        error: None,
    };
    serde_json::to_string(&msg).unwrap()
}

/// Send a request and receive the next message.
async fn call(
    ws: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    method: &str,
    params: serde_json::Value,
) -> ApiMessage {
    ws.send(Message::text(req(method, params))).await.unwrap();
    loop {
        let raw = ws.next().await.unwrap().unwrap();
        if let Message::Text(t) = raw {
            let msg: ApiMessage = serde_json::from_str(&t).unwrap();
            if msg.msg_type == MessageType::Response || msg.msg_type == MessageType::Error {
                return msg;
            }
            // Skip push messages until we get our response.
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_ping() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;
    let resp = call(&mut ws, "ping", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["pong"], true);
}

#[tokio::test]
async fn test_task_create_and_list() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create a task.
    let resp = call(&mut ws, "task.create", serde_json::json!({
        "title": "Fix the bug",
        "description": "It is broken",
        "tags": ["backend"]
    })).await;
    assert!(resp.error.is_none(), "create error: {:?}", resp.error);
    let task = resp.result.unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["title"], "Fix the bug");

    // List tasks.
    let resp = call(&mut ws, "task.list", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let tasks = resp.result.unwrap();
    let arr = tasks.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], task_id);
}

#[tokio::test]
async fn test_task_get() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.create", serde_json::json!({ "title": "Task A" })).await;
    let id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.get", serde_json::json!({ "id": id })).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["title"], "Task A");
}

#[tokio::test]
async fn test_task_get_next_and_complete() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    call(&mut ws, "task.create", serde_json::json!({ "title": "Task 1" })).await;

    // Claim next task.
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let task = resp.result.unwrap();
    assert!(!task.is_null(), "expected a task");
    assert_eq!(task["status"], "in-progress");
    let id = task["id"].as_str().unwrap().to_string();

    // Complete it.
    let resp = call(&mut ws, "task.complete", serde_json::json!({
        "id": id,
        "result": "done"
    })).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["completed"], id);
}

#[tokio::test]
async fn test_task_get_next_no_task() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    // When no task is available, result is null (which serde deserializes as None for Option<Value>).
    assert!(resp.result.is_none());
}

#[tokio::test]
async fn test_task_complete_returns_next_task() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create two tasks.
    let r1 = call(&mut ws, "task.create", serde_json::json!({ "title": "T1" })).await;
    let id1 = r1.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.create", serde_json::json!({ "title": "T2" })).await;

    // Claim first.
    call(&mut ws, "task.get_next", serde_json::json!({})).await;

    // Complete first — next_task should be T2.
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": id1 })).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["completed"], id1);
    let next = &result["next_task"];
    assert!(!next.is_null(), "next_task should be T2");
    assert_eq!(next["title"], "T2");
}

#[tokio::test]
async fn test_task_dependency_ordering() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r1 = call(&mut ws, "task.create", serde_json::json!({ "title": "Step 1" })).await;
    let id1 = r1.result.unwrap()["id"].as_str().unwrap().to_string();
    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "Step 2" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();

    // Step 2 depends on Step 1.
    let resp = call(&mut ws, "task.set_dependency", serde_json::json!({
        "task_id": id2,
        "depends_on_id": id1
    })).await;
    assert!(resp.error.is_none());

    // get_next should return Step 1 (no unmet deps).
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    let task = resp.result.unwrap();
    assert_eq!(task["title"], "Step 1");

    // Complete Step 1 — the complete response includes the next available task (Step 2).
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": id1 })).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let next = &result["next_task"];
    assert!(!next.is_null(), "next_task should be Step 2");
    assert_eq!(next["title"], "Step 2");
}

#[tokio::test]
async fn test_task_set_dependency_cycle_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r1 = call(&mut ws, "task.create", serde_json::json!({ "title": "A" })).await;
    let id1 = r1.result.unwrap()["id"].as_str().unwrap().to_string();
    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "B" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.set_dependency", serde_json::json!({
        "task_id": id2, "depends_on_id": id1
    })).await;

    // Creating a cycle (B → A already exists, now A → B) should fail.
    let resp = call(&mut ws, "task.set_dependency", serde_json::json!({
        "task_id": id1, "depends_on_id": id2
    })).await;
    assert!(resp.error.is_some(), "cycle should be rejected");
}

#[tokio::test]
async fn test_task_split() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Big Task" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    // Split with plain strings.
    let resp = call(&mut ws, "task.split", serde_json::json!({
        "id": id,
        "subtasks": ["Sub A", "Sub B"]
    })).await;
    assert!(resp.error.is_none(), "split error: {:?}", resp.error);
    let subs = resp.result.unwrap();
    assert_eq!(subs.as_array().unwrap().len(), 2);

    // Original task should be cancelled.
    let resp = call(&mut ws, "task.get", serde_json::json!({ "id": id })).await;
    assert_eq!(resp.result.unwrap()["status"], "cancelled");
}

#[tokio::test]
async fn test_topic_create_and_comment() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "topic.create", serde_json::json!({
        "title": "Discussion",
        "content": "Let's talk"
    })).await;
    assert!(resp.error.is_none());
    let topic_id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "topic.comment", serde_json::json!({
        "topic_id": topic_id,
        "content": "Good idea",
        "creator_agent_id": "agent-1"
    })).await;
    assert!(resp.error.is_none());

    let resp = call(&mut ws, "topic.get", serde_json::json!({ "id": topic_id })).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["comments"].as_array().unwrap().len(), 1);
    assert_eq!(result["comments"][0]["content"], "Good idea");
}

#[tokio::test]
async fn test_topic_list() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    call(&mut ws, "topic.create", serde_json::json!({ "title": "T1", "content": "" })).await;
    call(&mut ws, "topic.create", serde_json::json!({ "title": "T2", "content": "" })).await;

    let resp = call(&mut ws, "topic.list", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_push_send_and_ack() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "sender").await;
    let mut ws2 = connect(addr, "receiver").await;

    // Send a push message from sender to receiver.
    let resp = call(&mut ws1, "push.send", serde_json::json!({
        "to_agent_id": "receiver",
        "content": "hello!"
    })).await;
    assert!(resp.error.is_none());
    let msg_id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    // List pending messages for receiver.
    let resp = call(&mut ws2, "push.list", serde_json::json!({})).await;
    // The message may already be delivered (live delivery), or still pending.
    // Either way, no error.
    assert!(resp.error.is_none());

    // Ack the message.
    let resp = call(&mut ws2, "push.ack", serde_json::json!({
        "message_ids": [msg_id]
    })).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["acked"], 1);
}

#[tokio::test]
async fn test_agent_register_and_list() {
    let addr = start_server().await;
    let mut ws = connect(addr, "my-agent").await;

    let resp = call(&mut ws, "agent.register", serde_json::json!({
        "id": "my-agent",
        "name": "My Agent",
        "tags": ["backend"]
    })).await;
    assert!(resp.error.is_none());

    let resp = call(&mut ws, "agent.list", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let agents = resp.result.unwrap();
    let arr = agents.as_array().unwrap();
    assert!(arr.iter().any(|a| a["id"] == "my-agent"));
}

#[tokio::test]
async fn test_task_update() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Old Title" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.update", serde_json::json!({
        "id": id,
        "description": "Updated desc",
        "tags": ["new-tag"]
    })).await;
    assert!(resp.error.is_none());
    let task = resp.result.unwrap();
    assert_eq!(task["description"], "Updated desc");
    assert_eq!(task["tags"][0], "new-tag");
}

#[tokio::test]
async fn test_task_split_object_format() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Parent" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.split", serde_json::json!({
        "id": id,
        "subtasks": [
            { "title": "Sub 1", "description": "First sub", "tags": ["a"] },
            { "title": "Sub 2" }
        ]
    })).await;
    assert!(resp.error.is_none(), "split object format error: {:?}", resp.error);
    let subs = resp.result.unwrap();
    let arr = subs.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["title"], "Sub 1");
    assert_eq!(arr[0]["description"], "First sub");
}

// ── Internal test helpers ─────────────────────────────────────────────────────
// These re-export internal functions needed by tests. In Rust, integration tests
// in tests/ can only access pub items. We expose helpers via a test-only module
// below (using #[cfg(test)] is not available in tests/ dir, so this file uses
// the public API via re-exported symbols from the binary crate's lib target).
//
// Since hive-server has no lib target, we duplicate minimal setup here using
// the public types from hive_core and direct reqwest/WS calls.

mod hive_server_test_helpers {
    use hive_server::*;

    pub fn open_test_db() -> db::DbPool {
        let pool = db::open(":memory:").expect("open in-memory db");
        db::run_migrations(&pool).expect("migrations");
        pool
    }

    pub fn make_state(pool: db::DbPool) -> state::AppState {
        state::AppState::new(pool)
    }

    pub async fn serve(listener: tokio::net::TcpListener, state: state::AppState) {
        ws::serve(listener, state).await.unwrap();
    }
}
