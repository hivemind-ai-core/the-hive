//! Acceptance tests: server-initiated dispatch and live push delivery.
//!
//! These tests cover the gaps between the manual test plan (T1–T5, T7–T8)
//! and the existing acceptance tests. They verify:
//!
//! - **T1/T2**: Tagged task dispatch — a registered agent with matching tags
//!   receives a `task.assign` push when a matching task is created.
//! - **T3**: Untagged task dispatch — any registered agent receives an
//!   untagged task via `task.assign`.
//! - **T4/T5**: Live `push.notify` delivery — when a recipient is connected,
//!   `push.send` delivers a `push.notify` message over the WebSocket in real-time.
//! - **T7/T8**: Multi-agent topic collaboration — one agent creates a topic,
//!   another reads it, adds a comment, and creates their own topic.

use std::time::Duration;

use hive_acceptance::*;

// ── T1/T2: Server-initiated dispatch with tag matching ──────────────────────

// AT-T01: Registered agent with matching tag receives task.assign for a tagged task
#[tokio::test]
async fn tagged_task_dispatched_to_matching_registered_agent() {
    let addr = start_server().await;
    let mut ws_kilo = connect(addr, "agent-kilo-t01").await;
    let mut ws_claude = connect(addr, "agent-claude-t01").await;

    // Register both agents with different tags
    call(&mut ws_kilo, "agent.register", json!({
        "id": "agent-kilo-t01",
        "name": "Kilo Agent",
        "tags": ["kilo-only"]
    })).await;

    call(&mut ws_claude, "agent.register", json!({
        "id": "agent-claude-t01",
        "name": "Claude Agent",
        "tags": ["claude-only"]
    })).await;

    // Create a kilo-only task — should be dispatched to the kilo agent
    let _creator = connect(addr, "creator-t01").await;
    let mut ws_creator = connect(addr, "creator-t01b").await;
    call(&mut ws_creator, "task.create", json!({
        "title": "Kilo Task",
        "tags": ["kilo-only"]
    })).await;

    // Kilo should receive task.assign
    let push = recv_push_method(&mut ws_kilo, "task.assign", Duration::from_secs(3)).await;
    assert!(push.is_some(), "kilo agent should receive task.assign for kilo-tagged task");
    let task = &push.unwrap()["params"]["task"];
    assert_eq!(task["title"], "Kilo Task");

    // Claude should NOT receive this task (check briefly)
    let no_push = recv_push_method(&mut ws_claude, "task.assign", Duration::from_millis(500)).await;
    assert!(no_push.is_none(), "claude agent should NOT receive kilo-tagged task");
}

// AT-T02: Registered agent with "claude-only" tag receives task.assign for claude-tagged task
#[tokio::test]
async fn claude_tagged_task_dispatched_to_claude_agent() {
    let addr = start_server().await;
    let mut ws_kilo = connect(addr, "agent-kilo-t02").await;
    let mut ws_claude = connect(addr, "agent-claude-t02").await;

    call(&mut ws_kilo, "agent.register", json!({
        "id": "agent-kilo-t02",
        "name": "Kilo Agent",
        "tags": ["kilo-only"]
    })).await;

    call(&mut ws_claude, "agent.register", json!({
        "id": "agent-claude-t02",
        "name": "Claude Agent",
        "tags": ["claude-only"]
    })).await;

    // Create a claude-only task
    let mut ws_creator = connect(addr, "creator-t02").await;
    call(&mut ws_creator, "task.create", json!({
        "title": "Claude Task",
        "tags": ["claude-only"]
    })).await;

    // Claude should receive task.assign
    let push = recv_push_method(&mut ws_claude, "task.assign", Duration::from_secs(3)).await;
    assert!(push.is_some(), "claude agent should receive task.assign for claude-tagged task");
    let task = &push.unwrap()["params"]["task"];
    assert_eq!(task["title"], "Claude Task");

    // Kilo should NOT receive it
    let no_push = recv_push_method(&mut ws_kilo, "task.assign", Duration::from_millis(500)).await;
    assert!(no_push.is_none(), "kilo agent should NOT receive claude-tagged task");
}

// ── T3: Untagged task dispatched to any registered agent ────────────────────

// AT-T03: An untagged task is dispatched to a registered agent
#[tokio::test]
async fn untagged_task_dispatched_to_registered_agent() {
    let addr = start_server().await;
    let mut ws_agent = connect(addr, "agent-t03").await;

    call(&mut ws_agent, "agent.register", json!({
        "id": "agent-t03",
        "name": "Any Agent",
        "tags": ["general"]
    })).await;

    // Create an untagged task
    let mut ws_creator = connect(addr, "creator-t03").await;
    call(&mut ws_creator, "task.create", json!({
        "title": "Untagged Task"
    })).await;

    // Agent should receive task.assign (untagged tasks match any agent)
    let push = recv_push_method(&mut ws_agent, "task.assign", Duration::from_secs(3)).await;
    assert!(push.is_some(), "registered agent should receive task.assign for untagged task");
    let task = &push.unwrap()["params"]["task"];
    assert_eq!(task["title"], "Untagged Task");
}

// ── T4/T5: Live push.notify delivery ────────────────────────────────────────

// AT-T04: push.send delivers push.notify to a connected recipient in real-time
#[tokio::test]
async fn push_send_delivers_live_push_notify() {
    let addr = start_server().await;
    let mut ws_sender = connect(addr, "agent-t04-sender").await;
    let mut ws_recipient = connect(addr, "agent-t04-recipient").await;

    // Send a push while recipient is connected
    let res = call(&mut ws_sender, "push.send", json!({
        "to_agent_id": "agent-t04-recipient",
        "content": "Hello from sender!"
    })).await;
    assert!(res.error.is_none(), "push.send failed: {:?}", res.error);

    // Recipient should receive push.notify in real-time
    let push = recv_push_method(&mut ws_recipient, "push.notify", Duration::from_secs(3)).await;
    assert!(push.is_some(), "connected recipient should receive push.notify in real-time");

    let messages = &push.unwrap()["params"]["messages"];
    let messages = messages.as_array().expect("messages should be an array");
    assert!(!messages.is_empty(), "push.notify should contain at least one message");
    assert_eq!(messages[0]["content"], "Hello from sender!");
}

// AT-T05: push.send to a different connected agent — recipient gets push.notify
#[tokio::test]
async fn push_send_to_second_agent_delivers_live_push_notify() {
    let addr = start_server().await;
    let mut ws_sender = connect(addr, "agent-t05-sender").await;
    let mut ws_recipient = connect(addr, "agent-t05-recipient").await;

    let res = call(&mut ws_sender, "push.send", json!({
        "to_agent_id": "agent-t05-recipient",
        "content": "Message for agent B"
    })).await;
    assert!(res.error.is_none());

    let push = recv_push_method(&mut ws_recipient, "push.notify", Duration::from_secs(3)).await;
    assert!(push.is_some(), "second agent should receive push.notify");

    // Message is also available via push.list
    let res = call(&mut ws_recipient, "push.list", json!({})).await;
    let msgs = res.result.unwrap();
    let msgs = msgs.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"], "Message for agent B");
}

// ── T7/T8: Multi-agent topic collaboration ──────────────────────────────────

// AT-T07: Agent B reads Agent A's topic, adds comment, and creates own topic
#[tokio::test]
async fn multi_agent_topic_read_comment_create() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-t07a").await;
    let mut ws_b = connect(addr, "agent-t07b").await;

    // Agent A creates a topic
    let res = call(&mut ws_a, "topic.create", json!({
        "title": "Integration Test Topic",
        "content": "This topic tests the message board"
    })).await;
    assert!(res.error.is_none(), "topic.create failed: {:?}", res.error);
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Agent A adds a comment
    let res = call(&mut ws_a, "topic.comment", json!({
        "topic_id": &topic_id,
        "content": "Comment from agent A"
    })).await;
    assert!(res.error.is_none(), "comment failed: {:?}", res.error);

    // Agent B reads the topic
    let res = call(&mut ws_b, "topic.get", json!({"id": &topic_id})).await;
    assert!(res.error.is_none(), "topic.get failed: {:?}", res.error);
    let result = res.result.unwrap();
    assert_eq!(result["topic"]["title"], "Integration Test Topic");
    let comments = result["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["content"], "Comment from agent A");

    // Agent B adds their own comment
    let res = call(&mut ws_b, "topic.comment", json!({
        "topic_id": &topic_id,
        "content": "Agent B read this topic"
    })).await;
    assert!(res.error.is_none(), "agent B comment failed: {:?}", res.error);

    // Agent B creates their own topic
    let res = call(&mut ws_b, "topic.create", json!({
        "title": "Agent B Topic",
        "content": "Created by Agent B"
    })).await;
    assert!(res.error.is_none(), "agent B topic.create failed: {:?}", res.error);
    let b_topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Verify: original topic now has 2 comments
    let res = call(&mut ws_a, "topic.get", json!({"id": &topic_id})).await;
    let result = res.result.unwrap();
    let comments = result["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[1]["content"], "Agent B read this topic");

    // Verify: Agent B's topic exists and is readable by Agent A
    let res = call(&mut ws_a, "topic.get", json!({"id": &b_topic_id})).await;
    assert!(res.error.is_none());
    let result = res.result.unwrap();
    assert_eq!(result["topic"]["title"], "Agent B Topic");
    assert_eq!(result["topic"]["content"], "Created by Agent B");

    // Verify: topic.list shows both topics
    let res = call(&mut ws_a, "topic.list", json!({})).await;
    let topics = res.result.unwrap();
    let topics = topics.as_array().unwrap();
    assert!(topics.len() >= 2, "should have at least 2 topics");
    assert!(topics.iter().any(|t| t["title"] == "Integration Test Topic"));
    assert!(topics.iter().any(|t| t["title"] == "Agent B Topic"));
}

// AT-T08: Full round-trip — both agents create topics, comment on each other's,
// then both can see all content
#[tokio::test]
async fn two_agents_full_topic_round_trip() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-t08a").await;
    let mut ws_b = connect(addr, "agent-t08b").await;

    // Agent A creates topic
    let res = call(&mut ws_a, "topic.create", json!({
        "title": "Topic from A",
        "content": "A's discussion"
    })).await;
    let a_topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Agent B creates topic
    let res = call(&mut ws_b, "topic.create", json!({
        "title": "Topic from B",
        "content": "B's discussion"
    })).await;
    let b_topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Agent B comments on A's topic
    call(&mut ws_b, "topic.comment", json!({
        "topic_id": &a_topic_id,
        "content": "B commenting on A's topic"
    })).await;

    // Agent A comments on B's topic
    call(&mut ws_a, "topic.comment", json!({
        "topic_id": &b_topic_id,
        "content": "A commenting on B's topic"
    })).await;

    // Verify A's topic has B's comment
    let res = call(&mut ws_a, "topic.get", json!({"id": &a_topic_id})).await;
    let comments = res.result.unwrap()["comments"].as_array().unwrap().to_vec();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["content"], "B commenting on A's topic");

    // Verify B's topic has A's comment
    let res = call(&mut ws_b, "topic.get", json!({"id": &b_topic_id})).await;
    let comments = res.result.unwrap()["comments"].as_array().unwrap().to_vec();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["content"], "A commenting on B's topic");
}

// ── Full task lifecycle: create → dispatch → in-progress → done ─────────────

// AT-T09: Complete task lifecycle via server dispatch (not task.get_next)
#[tokio::test]
async fn full_task_lifecycle_via_dispatch() {
    let addr = start_server().await;
    let mut ws_agent = connect(addr, "agent-t09").await;

    // Register the agent (makes it eligible for try_dispatch)
    call(&mut ws_agent, "agent.register", json!({
        "id": "agent-t09",
        "name": "Lifecycle Agent",
        "tags": ["worker"]
    })).await;

    // Create a task — should be auto-dispatched
    let mut ws_creator = connect(addr, "creator-t09").await;
    let res = call(&mut ws_creator, "task.create", json!({
        "title": "Lifecycle Task",
        "tags": ["worker"]
    })).await;
    let task_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Agent receives task.assign
    let push = recv_push_method(&mut ws_agent, "task.assign", Duration::from_secs(3)).await;
    assert!(push.is_some(), "agent should receive task.assign");
    let assigned_task = &push.unwrap()["params"]["task"];
    assert_eq!(assigned_task["id"].as_str().unwrap(), task_id.as_str());
    assert_eq!(assigned_task["status"], "in-progress");

    // Agent completes the task
    let res = call(&mut ws_agent, "task.complete", json!({
        "id": &task_id,
        "result": "Task completed successfully"
    })).await;
    assert!(res.error.is_none(), "task.complete failed: {:?}", res.error);

    // Verify task is done
    let res = call(&mut ws_agent, "task.get", json!({"id": &task_id})).await;
    let task = res.result.unwrap();
    assert_eq!(task["status"], "done");
    assert_eq!(task["result"], "Task completed successfully");
}
