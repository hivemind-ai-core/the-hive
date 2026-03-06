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

// ── WS broadcast tests ───────────────────────────────────────────────────────

/// Read the next Push message from `ws` with a timeout. Skips non-Push messages.
async fn recv_push(
    ws: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    method: &str,
) -> ApiMessage {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let raw = ws.next().await.unwrap().unwrap();
            if let Message::Text(t) = raw {
                let msg: ApiMessage = serde_json::from_str(&t).unwrap();
                if msg.msg_type == MessageType::Push
                    && msg.method.as_deref() == Some(method)
                {
                    return msg;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for broadcast")
}

#[tokio::test]
async fn test_broadcast_tasks_updated_on_create() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "agent-1").await;
    let mut ws2 = connect(addr, "agent-2").await;

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    // Agent-1 creates a task.
    call(&mut ws1, "task.create", serde_json::json!({ "title": "Broadcast Task" })).await;

    // Agent-2 should receive a tasks.updated broadcast.
    let push = recv_push(&mut ws2, "tasks.updated").await;
    let tasks = push.params.unwrap();
    let arr = tasks.as_array().unwrap();
    assert!(arr.iter().any(|t| t["title"] == "Broadcast Task"), "tasks.updated must include the new task");
}

#[tokio::test]
async fn test_broadcast_topics_updated_on_comment() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "agent-1").await;
    let mut ws2 = connect(addr, "agent-2").await;

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let r = call(&mut ws1, "topic.create", serde_json::json!({ "title": "Disc", "content": "" })).await;
    let topic_id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    // Consume the topics.updated broadcast from topic.create on ws2.
    recv_push(&mut ws2, "topics.updated").await;

    // Agent-1 comments — another topics.updated broadcast.
    call(&mut ws1, "topic.comment", serde_json::json!({
        "topic_id": topic_id, "content": "hello", "creator_agent_id": "agent-1"
    })).await;

    let push = recv_push(&mut ws2, "topics.updated").await;
    let topics = push.params.unwrap();
    assert!(topics.as_array().unwrap().iter().any(|t| t["id"] == topic_id));
}

#[tokio::test]
async fn test_broadcast_agents_updated_on_register() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "agent-1").await;
    let mut ws2 = connect(addr, "agent-2").await;

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    // Agent-1 registers.
    call(&mut ws1, "agent.register", serde_json::json!({
        "id": "agent-1", "name": "Bot One", "tags": []
    })).await;

    // Agent-2 gets agents.updated broadcast.
    let push = recv_push(&mut ws2, "agents.updated").await;
    let agents = push.params.unwrap();
    assert!(agents.as_array().unwrap().iter().any(|a| a["id"] == "agent-1"));
}

// ── WS protocol error handling ───────────────────────────────────────────────

#[tokio::test]
async fn test_ws_unknown_method_returns_404_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "does.not.exist", serde_json::json!({})).await;
    assert_eq!(resp.msg_type, MessageType::Error, "unknown method must return Error type");
    assert_eq!(resp.error.unwrap().code, 404);
}

#[tokio::test]
async fn test_ws_malformed_json_does_not_break_connection() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Send garbage — server should log and ignore, NOT close the connection.
    ws.send(Message::text("this is not json at all {{{")).await.unwrap();

    // Give server a moment to process the garbage.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Connection should still work — a valid ping must succeed.
    let resp = call(&mut ws, "ping", serde_json::json!({})).await;
    assert!(resp.error.is_none(), "connection must remain functional after malformed message");
    assert_eq!(resp.result.unwrap()["pong"], true);
}

#[tokio::test]
async fn test_ws_missing_agent_id_drops_connection() {
    let addr = start_server().await;

    // Connect without agent_id query param — server should close the connection.
    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.unwrap();

    // The server drops the connection immediately. ws.next() should return None or an error.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws.next()
    ).await.expect("timed out waiting for server to close connection");

    // Either None (clean close) or Some(Ok(Close frame)) or Some(Err(...)).
    match result {
        None => {} // clean close
        Some(Ok(Message::Close(_))) => {} // explicit close frame
        Some(Ok(_)) => panic!("expected connection to be closed, not a data message"),
        Some(Err(_)) => {} // connection error is also acceptable
    }
}

// ── agent registry tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_register_upsert_semantics() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x").await;

    // First registration.
    let resp = call(&mut ws, "agent.register", serde_json::json!({
        "id": "agent-x", "name": "Old Name", "tags": []
    })).await;
    assert!(resp.error.is_none(), "first register should succeed");

    // Second registration with same ID — must update, not error.
    let resp = call(&mut ws, "agent.register", serde_json::json!({
        "id": "agent-x", "name": "New Name", "tags": ["backend"]
    })).await;
    assert!(resp.error.is_none(), "re-register must succeed (upsert semantics)");

    // agent.list should show updated name and tags.
    let resp = call(&mut ws, "agent.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    let agent = arr.as_array().unwrap().iter()
        .find(|a| a["id"] == "agent-x")
        .expect("agent-x must be in list");
    assert_eq!(agent["name"], "New Name");
    assert!(agent["tags"].as_array().unwrap().contains(&serde_json::json!("backend")));
}

#[tokio::test]
async fn test_agent_register_empty_id_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "agent.register", serde_json::json!({
        "id": "", "name": "Unnamed"
    })).await;
    assert!(resp.error.is_some(), "empty agent ID must be rejected");
}

#[tokio::test]
async fn test_agent_without_register_not_in_list() {
    // An agent that connects but never calls agent.register should NOT appear in agent.list.
    // touch_agent only UPDATEs — it requires a prior INSERT (from register) to work.
    let addr = start_server().await;
    let mut ws_unreg = connect(addr, "never-registered").await;
    let mut ws_reg = connect(addr, "registered").await;

    // Only the registered agent calls agent.register.
    call(&mut ws_reg, "agent.register", serde_json::json!({
        "id": "registered", "name": "Registered Agent", "tags": []
    })).await;

    // Trigger touch_agent by having the unregistered agent make any request.
    call(&mut ws_unreg, "ping", serde_json::json!({})).await;

    let resp = call(&mut ws_reg, "agent.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();

    assert!(arr.iter().any(|a| a["id"] == "registered"), "registered agent must appear");
    assert!(!arr.iter().any(|a| a["id"] == "never-registered"), "unregistered agent must NOT appear in list");
}

// ── topic.list_new timestamp filtering ───────────────────────────────────────

#[tokio::test]
async fn test_topic_list_new_timestamp_filtering() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Before any topics, list_new since epoch returns empty.
    let resp = call(&mut ws, "topic.list_new", serde_json::json!({ "since": 0 })).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 0);

    // Create Topic-A.
    let ra = call(&mut ws, "topic.create", serde_json::json!({ "title": "Topic-A", "content": "" })).await;
    let id_a = ra.result.unwrap()["id"].as_str().unwrap().to_string();

    // list_new since 0 now includes Topic-A.
    let resp = call(&mut ws, "topic.list_new", serde_json::json!({ "since": 0 })).await;
    let arr = resp.result.unwrap();
    assert!(arr.as_array().unwrap().iter().any(|t| t["id"] == id_a), "Topic-A should appear after since=0");

    // Record current timestamp, then create Topic-B after it.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await; // ensure > 1 second gap

    let rb = call(&mut ws, "topic.create", serde_json::json!({ "title": "Topic-B", "content": "" })).await;
    let id_b = rb.result.unwrap()["id"].as_str().unwrap().to_string();

    // list_new since now_secs should return Topic-B but NOT Topic-A.
    let resp = call(&mut ws, "topic.list_new", serde_json::json!({ "since": now_secs })).await;
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"] == id_b), "Topic-B should appear in list_new since T");
    assert!(!arr.iter().any(|t| t["id"] == id_a), "Topic-A should NOT appear in list_new since T");

    // Commenting on Topic-A bumps its last_updated_at — it should now appear.
    call(&mut ws, "topic.comment", serde_json::json!({
        "topic_id": id_a,
        "content": "bump",
        "creator_agent_id": "agent-1"
    })).await;
    let resp = call(&mut ws, "topic.list_new", serde_json::json!({ "since": now_secs })).await;
    let arr = resp.result.unwrap();
    assert!(arr.as_array().unwrap().iter().any(|t| t["id"] == id_a),
        "Topic-A should appear after comment bumps last_updated_at");

    // Far-future timestamp returns empty.
    let resp = call(&mut ws, "topic.list_new", serde_json::json!({ "since": 9_999_999_999u64 })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 0, "future timestamp should return empty");
}

// ── topic validation and ordering ────────────────────────────────────────────

#[tokio::test]
async fn test_topic_create_empty_title_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "topic.create", serde_json::json!({ "title": "", "content": "hi" })).await;
    assert!(resp.error.is_some(), "empty topic title must be rejected");
}

#[tokio::test]
async fn test_topic_get_nonexistent_returns_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "topic.get", serde_json::json!({ "id": "does-not-exist" })).await;
    assert!(resp.error.is_some(), "non-existent topic.get must return an error");
    assert!(resp.result.is_none());
}

#[tokio::test]
async fn test_topic_list_ordered_by_last_updated() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create Topic-A then Topic-B (B is newer).
    let ra = call(&mut ws, "topic.create", serde_json::json!({ "title": "Topic-A", "content": "" })).await;
    let id_a = ra.result.unwrap()["id"].as_str().unwrap().to_string();
    // Small sleep ensures distinct timestamps.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    call(&mut ws, "topic.create", serde_json::json!({ "title": "Topic-B", "content": "" })).await;

    // Without comments: B (newer) should be first.
    let resp = call(&mut ws, "topic.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr[0]["title"], "Topic-B", "newer topic should appear first");

    // Now comment on A — bumps A's last_updated_at.
    call(&mut ws, "topic.comment", serde_json::json!({
        "topic_id": id_a,
        "content": "bump!",
        "creator_agent_id": "agent-1"
    })).await;

    // Now A should be first (more recently updated).
    let resp = call(&mut ws, "topic.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr[0]["id"], id_a, "commented-on topic should now be first");
}

// ── push.ack edge cases ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_push_ack_idempotent() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Send a push to ourselves (offline — we won't read the WS push).
    let resp = call(&mut ws, "push.send", serde_json::json!({
        "to_agent_id": "agent-1",
        "content": "ack me twice"
    })).await;
    // May be live-delivered (we're connected), so grab the id regardless.
    let msg_id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    // Ack once.
    let resp = call(&mut ws, "push.ack", serde_json::json!({ "message_ids": [&msg_id] })).await;
    assert!(resp.error.is_none(), "first ack should succeed");

    // Ack again — must not error (idempotent UPDATE).
    let resp = call(&mut ws, "push.ack", serde_json::json!({ "message_ids": [&msg_id] })).await;
    assert!(resp.error.is_none(), "double-ack must be idempotent");
}

#[tokio::test]
async fn test_push_ack_empty_array() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "push.ack", serde_json::json!({ "message_ids": [] })).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["acked"], 0);
    assert_eq!(result["ok"], true);
}

#[tokio::test]
async fn test_push_ack_unknown_ids_no_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "push.ack", serde_json::json!({
        "message_ids": ["does-not-exist-at-all"]
    })).await;
    // The UPDATE affects 0 rows but should not error.
    assert!(resp.error.is_none(), "acking unknown IDs should be a no-op, not an error");
}

#[tokio::test]
async fn test_push_ack_missing_message_ids_returns_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "push.ack", serde_json::json!({})).await;
    assert!(resp.error.is_some(), "missing message_ids field must return an error");
}

// ── push delivery tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_push_live_delivery_marks_delivered() {
    // When receiver is connected, the push is delivered immediately over WS
    // and should NOT appear in push.list (already marked delivered).
    let addr = start_server().await;
    let mut sender = connect(addr, "sender").await;
    let mut receiver = connect(addr, "receiver").await;

    // Give the server a moment to register both connections.
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let resp = call(&mut sender, "push.send", serde_json::json!({
        "to_agent_id": "receiver",
        "content": "live hello"
    })).await;
    assert!(resp.error.is_none());
    let msg_id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    // Receiver should get a Push-type WS message.
    let push_received = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async {
            loop {
                let raw = receiver.next().await.unwrap().unwrap();
                if let Message::Text(t) = raw {
                    let msg: ApiMessage = serde_json::from_str(&t).unwrap();
                    if msg.msg_type == MessageType::Push {
                        return msg;
                    }
                }
            }
        }
    ).await.expect("timed out waiting for push WS notification");

    assert_eq!(push_received.params.unwrap()["content"], "live hello");

    // push.list should be empty — already delivered.
    let resp = call(&mut receiver, "push.list", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let still_pending = arr.as_array().unwrap().iter().any(|m| m["id"] == msg_id);
    assert!(!still_pending, "live-delivered message must not appear in push.list");
}

#[tokio::test]
async fn test_push_offline_delivery_flow() {
    // Send a push before receiver connects — stored in DB.
    // Receiver retrieves it via push.list then acks it.
    let addr = start_server().await;
    let mut sender = connect(addr, "sender").await;

    let resp = call(&mut sender, "push.send", serde_json::json!({
        "to_agent_id": "late-receiver",
        "content": "stored for later"
    })).await;
    assert!(resp.error.is_none());
    let msg_id = resp.result.unwrap()["id"].as_str().unwrap().to_string();

    // Receiver connects after the fact.
    let mut receiver = connect(addr, "late-receiver").await;

    // push.list should contain the stored message.
    let resp = call(&mut receiver, "push.list", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert!(arr.iter().any(|m| m["id"] == msg_id), "stored message must appear in push.list");
    assert_eq!(arr.iter().find(|m| m["id"] == msg_id).unwrap()["content"], "stored for later");

    // Ack it.
    let resp = call(&mut receiver, "push.ack", serde_json::json!({ "message_ids": [&msg_id] })).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["acked"], 1);

    // push.list now empty.
    let resp = call(&mut receiver, "push.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    assert!(!arr.as_array().unwrap().iter().any(|m| m["id"] == msg_id), "acked message must be gone");
}

// ── task.split: chain, non-existent parent, empty subtasks ───────────────────

#[tokio::test]
async fn test_task_split_chain_enforced() {
    // Splitting into [Sub1, Sub2, Sub3] must create a sequential dependency chain:
    // Sub2 depends on Sub1, Sub3 depends on Sub2.
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Big Task" })).await;
    let parent_id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.split", serde_json::json!({
        "id": parent_id,
        "subtasks": ["Sub1", "Sub2", "Sub3"]
    })).await;
    assert!(resp.error.is_none());
    let subs = resp.result.unwrap();
    let arr = subs.as_array().unwrap();
    let id1 = arr[0]["id"].as_str().unwrap().to_string();
    let id2 = arr[1]["id"].as_str().unwrap().to_string();
    let id3 = arr[2]["id"].as_str().unwrap().to_string();

    // Sub1 is available first.
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert_eq!(resp.result.unwrap()["id"], id1, "Sub1 should be first");

    // Complete Sub1 — Sub2 becomes available (auto-claimed), Sub3 still blocked.
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": id1 })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert_eq!(next["id"], id2, "Sub2 should be next after Sub1");

    // Complete Sub2 — Sub3 becomes available.
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": id2 })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert_eq!(next["id"], id3, "Sub3 should be last after Sub2");
}

#[tokio::test]
async fn test_task_split_nonexistent_parent_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.split", serde_json::json!({
        "id": "does-not-exist",
        "subtasks": ["Sub A"]
    })).await;
    assert!(resp.error.is_some(), "split on non-existent parent must return error");
}

#[tokio::test]
async fn test_task_split_empty_subtasks_cancels_parent() {
    // Current behavior: empty subtasks array cancels the parent with no replacements.
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Doomed Task" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.split", serde_json::json!({ "id": id, "subtasks": [] })).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 0);

    // Parent is cancelled.
    let resp = call(&mut ws, "task.get", serde_json::json!({ "id": id })).await;
    assert_eq!(resp.result.unwrap()["status"], "cancelled");

    // No new tasks created.
    let resp = call(&mut ws, "task.list", serde_json::json!({})).await;
    let arr = resp.result.unwrap();
    assert_eq!(arr.as_array().unwrap().len(), 1, "only the original (cancelled) task exists");
}

// ── task.set_dependency: self-dep rejection and position ordering ─────────────

#[tokio::test]
async fn test_set_dependency_self_dep_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Self" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    let resp = call(&mut ws, "task.set_dependency", serde_json::json!({
        "task_id": id,
        "depends_on_id": id
    })).await;
    assert!(resp.error.is_some(), "self-dependency must be rejected");
}

#[tokio::test]
async fn test_set_dependency_positions_are_correct() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create A, B, C — then set B depends on A, C depends on B.
    let ra = call(&mut ws, "task.create", serde_json::json!({ "title": "A" })).await;
    let id_a = ra.result.unwrap()["id"].as_str().unwrap().to_string();
    let rb = call(&mut ws, "task.create", serde_json::json!({ "title": "B" })).await;
    let id_b = rb.result.unwrap()["id"].as_str().unwrap().to_string();
    let rc = call(&mut ws, "task.create", serde_json::json!({ "title": "C" })).await;
    let id_c = rc.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_b, "depends_on_id": id_a })).await;
    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_c, "depends_on_id": id_b })).await;

    // Read positions via task.get and verify strict ordering: pos[A] < pos[B] < pos[C].
    let ta = call(&mut ws, "task.get", serde_json::json!({ "id": id_a })).await.result.unwrap();
    let tb = call(&mut ws, "task.get", serde_json::json!({ "id": id_b })).await.result.unwrap();
    let tc = call(&mut ws, "task.get", serde_json::json!({ "id": id_c })).await.result.unwrap();

    let pos_a = ta["position"].as_f64().unwrap();
    let pos_b = tb["position"].as_f64().unwrap();
    let pos_c = tc["position"].as_f64().unwrap();

    assert!(pos_a < pos_b, "A must come before B: {pos_a} < {pos_b}");
    assert!(pos_b < pos_c, "B must come before C: {pos_b} < {pos_c}");
}

// ── task.get_next: all dependencies must be done ─────────────────────────────

#[tokio::test]
async fn test_get_next_fan_in_requires_all_deps_done() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create A, B, C where C depends on both A and B.
    let ra = call(&mut ws, "task.create", serde_json::json!({ "title": "A" })).await;
    let id_a = ra.result.unwrap()["id"].as_str().unwrap().to_string();
    let rb = call(&mut ws, "task.create", serde_json::json!({ "title": "B" })).await;
    let id_b = rb.result.unwrap()["id"].as_str().unwrap().to_string();
    let rc = call(&mut ws, "task.create", serde_json::json!({ "title": "C" })).await;
    let id_c = rc.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_c, "depends_on_id": id_a })).await;
    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_c, "depends_on_id": id_b })).await;

    // Complete A first.
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    let first = resp.result.unwrap();
    assert!(first["id"] == id_a || first["id"] == id_b, "first should be A or B");
    let first_id = first["id"].as_str().unwrap().to_string();
    let second_id = if first_id == id_a { id_b.clone() } else { id_a.clone() };

    // Complete first dep. next_task from auto-claim should be the OTHER dep (not C).
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": first_id })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    // The auto-claimed next should be the second dep (not C, since C still has one unmet dep).
    assert!(!next.is_null(), "auto-claim should find the second dep");
    assert_eq!(next["id"], second_id, "second dep should be claimed, not C");

    // Now complete the second dep. C should become available as next_task.
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": second_id })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert!(!next.is_null(), "C should now be auto-claimed after both deps done");
    assert_eq!(next["id"], id_c);
}

#[tokio::test]
async fn test_get_next_diamond_dependency() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Diamond: root → A, root → B, C → A, C → B
    // Order: Root first, then A and B (both unlocked), then C (both A and B must be done).
    let r_root = call(&mut ws, "task.create", serde_json::json!({ "title": "Root" })).await;
    let id_root = r_root.result.unwrap()["id"].as_str().unwrap().to_string();
    let r_a = call(&mut ws, "task.create", serde_json::json!({ "title": "Mid-A" })).await;
    let id_a = r_a.result.unwrap()["id"].as_str().unwrap().to_string();
    let r_b = call(&mut ws, "task.create", serde_json::json!({ "title": "Mid-B" })).await;
    let id_b = r_b.result.unwrap()["id"].as_str().unwrap().to_string();
    let r_c = call(&mut ws, "task.create", serde_json::json!({ "title": "Final-C" })).await;
    let id_c = r_c.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_a, "depends_on_id": id_root })).await;
    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_b, "depends_on_id": id_root })).await;
    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_c, "depends_on_id": id_a })).await;
    call(&mut ws, "task.set_dependency", serde_json::json!({ "task_id": id_c, "depends_on_id": id_b })).await;

    // Only Root is available now.
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert_eq!(resp.result.unwrap()["id"], id_root, "Root should be first");

    // Complete Root. Mid-A or Mid-B should be auto-claimed (whichever comes first by position).
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": id_root })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert!(!next.is_null(), "a mid task should be available after Root done");
    let first_mid = next["id"].as_str().unwrap().to_string();
    assert!(first_mid == id_a || first_mid == id_b, "first mid should be A or B");
    let second_mid = if first_mid == id_a { id_b.clone() } else { id_a.clone() };

    // Complete first mid. auto-claim grabs second mid (C still has unmet dep).
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": first_mid })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert!(!next.is_null(), "second mid should be available");
    assert_eq!(next["id"], second_mid, "second mid should be next, not C");

    // Complete second mid. Now C is available.
    let resp = call(&mut ws, "task.complete", serde_json::json!({ "id": second_mid })).await;
    let next = resp.result.unwrap()["next_task"].clone();
    assert!(!next.is_null(), "C should be available after both mids done");
    assert_eq!(next["id"], id_c, "C is the final task");
}

// ── task.get_next skips non-pending tasks ────────────────────────────────────

#[tokio::test]
async fn test_get_next_skips_in_progress_tasks() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "agent-1").await;
    let mut ws2 = connect(addr, "agent-2").await;

    // Only one task exists; agent-1 claims it.
    call(&mut ws1, "task.create", serde_json::json!({ "title": "Claimed Task" })).await;
    call(&mut ws1, "task.get_next", serde_json::json!({})).await;

    // agent-2 calls get_next — nothing is available (task is in-progress).
    let resp = call(&mut ws2, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    assert!(resp.result.is_none(), "in-progress task should not be returned to another agent");
}

#[tokio::test]
async fn test_get_next_skips_blocked_tasks() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Blocked Task" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();

    // Block the task.
    call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "blocked" })).await;

    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    assert!(resp.result.is_none(), "blocked task should not be returned by get_next");
}

#[tokio::test]
async fn test_get_next_skips_cancelled_and_done_tasks() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create two tasks: one cancelled, one done (via in-progress→done path).
    let r1 = call(&mut ws, "task.create", serde_json::json!({ "title": "Cancelled" })).await;
    let id1 = r1.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.update", serde_json::json!({ "id": id1, "status": "cancelled" })).await;

    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "Done" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.get_next", serde_json::json!({})).await; // claims Done task
    call(&mut ws, "task.update", serde_json::json!({ "id": id2, "status": "done" })).await;

    // No pending tasks remain — get_next returns null.
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    assert!(resp.result.is_none(), "cancelled/done tasks should not be returned by get_next");
}

#[tokio::test]
async fn test_get_next_returns_pending_after_skipping_non_pending() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create: one blocked, one cancelled, one pending.
    let r1 = call(&mut ws, "task.create", serde_json::json!({ "title": "Blocked" })).await;
    let id1 = r1.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.update", serde_json::json!({ "id": id1, "status": "blocked" })).await;

    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "Cancelled" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.update", serde_json::json!({ "id": id2, "status": "cancelled" })).await;

    call(&mut ws, "task.create", serde_json::json!({ "title": "The One" })).await;

    // get_next should skip blocked and cancelled, return "The One".
    let resp = call(&mut ws, "task.get_next", serde_json::json!({})).await;
    assert!(resp.error.is_none());
    let task = resp.result.unwrap();
    assert_eq!(task["title"], "The One", "should skip non-pending tasks and return the pending one");
    assert_eq!(task["status"], "in-progress");
}

// ── Status transition tests ──────────────────────────────────────────────────

#[tokio::test]
async fn test_task_update_valid_transitions() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // pending → blocked
    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "T" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "blocked" })).await;
    assert!(resp.error.is_none(), "pending→blocked should succeed");
    assert_eq!(resp.result.unwrap()["status"], "blocked");

    // blocked → pending
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "pending" })).await;
    assert!(resp.error.is_none(), "blocked→pending should succeed");
    assert_eq!(resp.result.unwrap()["status"], "pending");

    // pending → cancelled
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "cancelled" })).await;
    assert!(resp.error.is_none(), "pending→cancelled should succeed");
    assert_eq!(resp.result.unwrap()["status"], "cancelled");

    // in-progress → done (via get_next to get into in-progress)
    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "T2" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.get_next", serde_json::json!({})).await; // makes T2 in-progress
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id2, "status": "done" })).await;
    assert!(resp.error.is_none(), "in-progress→done should succeed");
    assert_eq!(resp.result.unwrap()["status"], "done");
}

#[tokio::test]
async fn test_task_update_invalid_transitions() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // pending → done is INVALID
    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "T" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "done" })).await;
    assert!(resp.error.is_some(), "pending→done should be rejected");

    // in-progress → pending is INVALID
    call(&mut ws, "task.get_next", serde_json::json!({})).await;
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "pending" })).await;
    assert!(resp.error.is_some(), "in-progress→pending should be rejected");
}

#[tokio::test]
async fn test_task_update_terminal_states_reject_all_transitions() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Make a task done: pending → in-progress → done.
    let r = call(&mut ws, "task.create", serde_json::json!({ "title": "Done Task" })).await;
    let id = r.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.get_next", serde_json::json!({})).await;
    call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "done" })).await;

    // done → pending
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "pending" })).await;
    assert!(resp.error.is_some(), "done→pending should be rejected");

    // done → cancelled
    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id, "status": "cancelled" })).await;
    assert!(resp.error.is_some(), "done→cancelled should be rejected");

    // Make a task cancelled, verify it also rejects transitions.
    let r2 = call(&mut ws, "task.create", serde_json::json!({ "title": "Cancelled Task" })).await;
    let id2 = r2.result.unwrap()["id"].as_str().unwrap().to_string();
    call(&mut ws, "task.update", serde_json::json!({ "id": id2, "status": "cancelled" })).await;

    let resp = call(&mut ws, "task.update", serde_json::json!({ "id": id2, "status": "pending" })).await;
    assert!(resp.error.is_some(), "cancelled→pending should be rejected");
}

// ── Validation tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_task_create_empty_title_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.create", serde_json::json!({ "title": "" })).await;
    assert!(resp.error.is_some(), "empty title should be rejected");
}

#[tokio::test]
async fn test_task_create_whitespace_title_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.create", serde_json::json!({ "title": "   " })).await;
    assert!(resp.error.is_some(), "whitespace-only title should be rejected");
}

#[tokio::test]
async fn test_task_get_unknown_id_returns_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    let resp = call(&mut ws, "task.get", serde_json::json!({ "id": "does-not-exist" })).await;
    assert!(resp.error.is_some(), "unknown task ID should return an error, not null");
    assert!(resp.result.is_none());
}

// ── task.list filter tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_task_list_status_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create two tasks; claim one (making it in-progress).
    call(&mut ws, "task.create", serde_json::json!({ "title": "Pending Task" })).await;
    call(&mut ws, "task.create", serde_json::json!({ "title": "In Progress Task" })).await;
    call(&mut ws, "task.get_next", serde_json::json!({})).await; // claims one

    // Filter by pending — should return exactly one.
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "pending" })).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one pending task");
    assert_eq!(arr[0]["status"], "pending");

    // Filter by in-progress — should return exactly one.
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "in-progress" })).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one in-progress task");
    assert_eq!(arr[0]["status"], "in-progress");

    // No filter — returns both.
    let resp = call(&mut ws, "task.list", serde_json::json!({})).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_task_list_tag_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    call(&mut ws, "task.create", serde_json::json!({ "title": "Backend Task", "tags": ["backend"] })).await;
    call(&mut ws, "task.create", serde_json::json!({ "title": "Frontend Task", "tags": ["frontend"] })).await;
    call(&mut ws, "task.create", serde_json::json!({ "title": "Full Stack", "tags": ["backend", "frontend"] })).await;

    // Filter by "backend" — should match tasks 1 and 3.
    let resp = call(&mut ws, "task.list", serde_json::json!({ "tag": "backend" })).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2, "expected 2 backend tasks");
    assert!(arr.iter().all(|t| t["tags"].as_array().unwrap().contains(&serde_json::json!("backend"))));

    // Filter by "frontend" — should match tasks 2 and 3.
    let resp = call(&mut ws, "task.list", serde_json::json!({ "tag": "frontend" })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 2);

    // Filter by non-existent tag — empty result.
    let resp = call(&mut ws, "task.list", serde_json::json!({ "tag": "nonexistent" })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_task_list_agent_filter() {
    let addr = start_server().await;
    let mut ws1 = connect(addr, "agent-alpha").await;
    let mut ws2 = connect(addr, "agent-beta").await;

    // Create two tasks — each agent claims one.
    call(&mut ws1, "task.create", serde_json::json!({ "title": "Alpha's Task" })).await;
    call(&mut ws1, "task.create", serde_json::json!({ "title": "Beta's Task" })).await;

    call(&mut ws1, "task.get_next", serde_json::json!({})).await; // agent-alpha claims first
    call(&mut ws2, "task.get_next", serde_json::json!({})).await; // agent-beta claims second

    // Filter by agent-alpha — exactly one task.
    let resp = call(&mut ws1, "task.list", serde_json::json!({ "assigned_agent_id": "agent-alpha" })).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["assigned_agent_id"], "agent-alpha");

    // Filter by agent-beta — exactly one task.
    let resp = call(&mut ws1, "task.list", serde_json::json!({ "assigned_agent_id": "agent-beta" })).await;
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["assigned_agent_id"], "agent-beta");
}

#[tokio::test]
async fn test_task_list_combined_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-1").await;

    // Create tasks: two backend (one will be claimed as in-progress), one frontend (stays pending).
    call(&mut ws, "task.create", serde_json::json!({ "title": "Backend A", "tags": ["backend"] })).await;
    call(&mut ws, "task.create", serde_json::json!({ "title": "Backend B", "tags": ["backend"] })).await;
    call(&mut ws, "task.create", serde_json::json!({ "title": "Frontend A", "tags": ["frontend"] })).await;

    // Claim one backend task — it becomes in-progress.
    call(&mut ws, "task.get_next", serde_json::json!({ "tag": "backend" })).await;

    // status=pending + tag=backend → 1 (Backend B is still pending)
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "pending", "tag": "backend" })).await;
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1, "one backend task is still pending");
    assert_eq!(arr[0]["tags"].as_array().unwrap().contains(&serde_json::json!("backend")), true);

    // status=in-progress + tag=backend → 1 (Backend A was just claimed)
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "in-progress", "tag": "backend" })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 1);

    // status=pending + tag=frontend → 1 (Frontend A, untouched)
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "pending", "tag": "frontend" })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 1);

    // status=in-progress + tag=frontend → 0
    let resp = call(&mut ws, "task.list", serde_json::json!({ "status": "in-progress", "tag": "frontend" })).await;
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 0);
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
