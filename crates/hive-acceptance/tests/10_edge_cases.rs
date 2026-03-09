//! Acceptance tests: edge cases and boundary conditions.
//!
//! Covers: dispatch tag semantics, agent re-register resetting tasks,
//! task.complete result storage, topic.list_new since filter,
//! push.ack batch, topic.wait timeout, @mention notifications,
//! multi-filter task.list.

use hive_acceptance::*;

// ── Task dispatch tag semantics ───────────────────────────────────────────────

// AT-X01: An agent with no tag filter can claim any pending task including tagged ones
#[tokio::test]
async fn untagged_agent_claims_any_task() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x01").await;

    call(&mut ws, "task.create", json!({"title": "Tagged", "tags": ["rust"]})).await;

    // No tag filter → gets any task.
    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert!(res.error.is_none());
    let task = res.result.unwrap();
    assert!(!task.is_null(), "untagged agent should claim a tagged task");
    assert_eq!(task["title"], "Tagged");
}

// AT-X02: Typed agent with tag "rust" can claim untagged tasks
#[tokio::test]
async fn rust_tagged_agent_claims_untagged_task() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x02").await;

    call(&mut ws, "task.create", json!({"title": "No Tags"})).await;

    let res = call(&mut ws, "task.get_next", json!({"tag": "rust"})).await;
    assert!(res.error.is_none());
    let task = res.result.unwrap();
    assert!(!task.is_null(), "rust agent should claim untagged task");
}

// AT-X03: A tagged task is NOT claimed by a differently tagged agent
#[tokio::test]
async fn tagged_task_not_claimed_by_different_tag_agent() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x03").await;

    call(&mut ws, "task.create", json!({"title": "Python Only", "tags": ["python"]})).await;

    // Agent with "rust" tag should NOT claim a "python"-only task.
    let res = call(&mut ws, "task.get_next", json!({"tag": "rust"})).await;
    assert!(res.error.is_none());
    assert!(
        res.result.is_none(),
        "rust agent should not claim a python-tagged task"
    );
}

// ── Agent re-register resets orphaned tasks ───────────────────────────────────

// AT-X04: When an agent re-registers, its in-progress tasks are reset to pending.
// Since the agent is still connected and registered (capacity_max=1), one task
// gets immediately re-dispatched. With 2 tasks claimed, after re-register there
// will be 1 in-progress and 1 pending.
#[tokio::test]
async fn agent_reregister_resets_in_progress_tasks() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x04").await;

    // Create 2 tasks and claim both via task.get_next.
    call(&mut ws, "task.create", json!({"title": "Task 1"})).await;
    call(&mut ws, "task.create", json!({"title": "Task 2"})).await;
    call(&mut ws, "task.get_next", json!({})).await;
    call(&mut ws, "task.get_next", json!({})).await;

    // Confirm both are in-progress.
    let res = call(&mut ws, "task.list", json!({"status": "in-progress"})).await;
    assert_eq!(res.result.unwrap().as_array().unwrap().len(), 2);

    // Re-register: resets both tasks to pending, then try_dispatch claims 1 (capacity=1).
    let res = call(&mut ws, "agent.register", json!({
        "id": "agent-x04",
        "name": "Agent X04"
    })).await;
    assert!(res.error.is_none(), "register failed: {:?}", res.error);

    // After reset + re-dispatch: 1 pending, 1 in-progress.
    let res = call(&mut ws, "task.list", json!({"status": "pending"})).await;
    assert_eq!(
        res.result.unwrap().as_array().unwrap().len(),
        1,
        "one task should remain pending after re-register (the other was re-dispatched)"
    );
}

// ── task.complete stores result ───────────────────────────────────────────────

// AT-X05: task.complete with result string stores it on the task
#[tokio::test]
async fn task_complete_stores_result_string() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x05").await;

    call(&mut ws, "task.create", json!({"title": "Has Result"})).await;
    let claim = call(&mut ws, "task.get_next", json!({})).await;
    let id = claim.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.complete", json!({
        "id": &id,
        "result": "Output: success"
    })).await;

    let res = call(&mut ws, "task.get", json!({"id": &id})).await;
    let task = res.result.unwrap();
    assert_eq!(task["status"], "done");
    assert_eq!(task["result"], "Output: success");
}

// AT-X06: task.complete when next task is blocked returns null next_task
#[tokio::test]
async fn task_complete_next_task_null_if_all_blocked() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x06").await;

    let a = call(&mut ws, "task.create", json!({"title": "A"})).await.result.unwrap();
    let b = call(&mut ws, "task.create", json!({"title": "B"})).await.result.unwrap();
    let c = call(&mut ws, "task.create", json!({"title": "C"})).await.result.unwrap();
    let b_id = b["id"].as_str().unwrap();
    let c_id = c["id"].as_str().unwrap();

    // B depends on A, C depends on B.
    call(&mut ws, "task.set_dependency", json!({"task_id": b_id, "depends_on_id": a["id"].as_str().unwrap()})).await;
    call(&mut ws, "task.set_dependency", json!({"task_id": c_id, "depends_on_id": b_id})).await;

    // Claim A.
    let claim = call(&mut ws, "task.get_next", json!({})).await;
    let a_id = claim.result.unwrap()["id"].as_str().unwrap().to_string();

    // Complete A — B is now unblocked.
    let res = call(&mut ws, "task.complete", json!({"id": &a_id})).await;
    let r = res.result.unwrap();
    // next_task should be B (not null) because completing A unblocks B.
    let next = &r["next_task"];
    assert!(!next.is_null(), "next_task should be B after A is done");
    assert_eq!(next["title"], "B");
}

// ── topic.list_new ────────────────────────────────────────────────────────────

// AT-X07: topic.list_new with since=0 returns all topics
#[tokio::test]
async fn topic_list_new_since_zero_returns_all() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x07").await;

    call(&mut ws, "topic.create", json!({"title": "T1", "content": "c"})).await;
    call(&mut ws, "topic.create", json!({"title": "T2", "content": "c"})).await;

    let res = call(&mut ws, "topic.list_new", json!({"since": 0})).await;
    assert!(res.error.is_none());
    let topics = res.result.unwrap();
    assert_eq!(topics.as_array().unwrap().len(), 2);
}

// AT-X08: topic.list_new with future timestamp returns empty
#[tokio::test]
async fn topic_list_new_future_since_returns_empty() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x08").await;

    call(&mut ws, "topic.create", json!({"title": "Old", "content": "c"})).await;

    let far_future = 9_999_999_999i64;
    let res = call(&mut ws, "topic.list_new", json!({"since": far_future})).await;
    assert!(res.error.is_none());
    let topics = res.result.unwrap();
    assert!(topics.as_array().unwrap().is_empty());
}

// ── topic.wait timeout ────────────────────────────────────────────────────────

// AT-X09: topic.wait returns error when timeout expires with no new comments
#[tokio::test]
async fn topic_wait_times_out_with_no_comments() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x09").await;

    let res = call(&mut ws, "topic.create", json!({"title": "Waiting", "content": "c"})).await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Wait 1 second with no comments — should time out.
    let res = call(&mut ws, "topic.wait", json!({
        "id": &topic_id,
        "since_count": 0,
        "timeout_secs": 1
    })).await;
    assert!(res.error.is_some(), "topic.wait should error on timeout");
    let err_msg = res.error.unwrap().to_string();
    assert!(err_msg.contains("timeout"), "error should mention timeout: {err_msg}");
}

// ── push.ack batch ────────────────────────────────────────────────────────────

// AT-X10: push.ack with multiple IDs marks all as delivered
#[tokio::test]
async fn push_ack_batch_marks_all_delivered() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-x10a").await;

    // Send two messages to B.
    let r1 = call(&mut ws_a, "push.send", json!({"to_agent_id": "agent-x10b", "content": "msg1"}))
        .await.result.unwrap();
    let r2 = call(&mut ws_a, "push.send", json!({"to_agent_id": "agent-x10b", "content": "msg2"}))
        .await.result.unwrap();
    let id1 = r1["id"].as_str().unwrap();
    let id2 = r2["id"].as_str().unwrap();

    let mut ws_b = connect(addr, "agent-x10b").await;

    // B sees both.
    let res = call(&mut ws_b, "push.list", json!({})).await;
    assert_eq!(res.result.unwrap().as_array().unwrap().len(), 2);

    // B acks both in one call.
    let res = call(&mut ws_b, "push.ack", json!({"message_ids": [id1, id2]})).await;
    assert!(res.error.is_none(), "batch ack failed: {:?}", res.error);
    let r = res.result.unwrap();
    assert_eq!(r["acked"], 2);

    // B's list is now empty.
    let res = call(&mut ws_b, "push.list", json!({})).await;
    assert!(res.result.unwrap().as_array().unwrap().is_empty());
}

// AT-X11: push.ack with empty array is ok and acks 0 messages
#[tokio::test]
async fn push_ack_empty_array_is_ok() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x11").await;

    let res = call(&mut ws, "push.ack", json!({"message_ids": []})).await;
    assert!(res.error.is_none());
    let r = res.result.unwrap();
    assert_eq!(r["acked"], 0);
}

// ── @mention notifications ────────────────────────────────────────────────────

// AT-X12: @mention in comment sends push notification to mentioned agent
#[tokio::test]
async fn mention_in_comment_sends_push_to_recipient() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-x12a").await;

    let res = call(&mut ws_a, "topic.create", json!({
        "title": "Discussion",
        "content": "Start"
    })).await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // A comments @mention of B.
    call(&mut ws_a, "topic.comment", json!({
        "topic_id": &topic_id,
        "content": "Hey @agent-x12b check this out!"
    })).await;

    // B connects and should find a push notification.
    let mut ws_b = connect(addr, "agent-x12b").await;
    let res = call(&mut ws_b, "push.list", json!({})).await;
    let msgs = res.result.unwrap();
    let msgs = msgs.as_array().unwrap();
    assert_eq!(msgs.len(), 1, "mentioned agent should receive a push notification");
    assert!(
        msgs[0]["content"].as_str().unwrap().contains("tagged"),
        "notification should mention 'tagged': {}",
        msgs[0]["content"]
    );
}

// AT-X13: Self-mention does not send a push notification
#[tokio::test]
async fn self_mention_does_not_send_push() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x13").await;

    let res = call(&mut ws, "topic.create", json!({"title": "T", "content": "c"})).await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Agent mentions itself — must include creator_agent_id for the server to detect the self-mention.
    call(&mut ws, "topic.comment", json!({
        "topic_id": &topic_id,
        "content": "I am @agent-x13 and I'm talking to myself",
        "creator_agent_id": "agent-x13"
    })).await;

    let res = call(&mut ws, "push.list", json!({})).await;
    assert!(
        res.result.unwrap().as_array().unwrap().is_empty(),
        "self-mention should not generate a push notification"
    );
}

// ── task.list multi-filter ────────────────────────────────────────────────────

// AT-X14: task.list with status + tag filter combined
#[tokio::test]
async fn task_list_status_and_tag_combined() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x14").await;

    // "First Rust" is created first (lowest position) and will be claimed.
    // "Second Rust" remains pending. "Python Task" is never affected by rust-tag filter.
    call(&mut ws, "task.create", json!({"title": "First Rust", "tags": ["rust"]})).await;
    call(&mut ws, "task.create", json!({"title": "Python Task", "tags": ["python"]})).await;
    call(&mut ws, "task.create", json!({"title": "Second Rust", "tags": ["rust"]})).await;

    // Claim the first available rust task (= "First Rust").
    call(&mut ws, "task.get_next", json!({"tag": "rust"})).await;

    // Filter: pending + rust → only "Second Rust" remains pending.
    let res = call(&mut ws, "task.list", json!({"status": "pending", "tag": "rust"})).await;
    let tasks = res.result.unwrap();
    let tasks = tasks.as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["title"], "Second Rust");
}

// AT-X15: task.list with assigned_agent_id filter
#[tokio::test]
async fn task_list_filter_by_assigned_agent() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-x15a").await;
    let mut ws_b = connect(addr, "agent-x15b").await;

    call(&mut ws_a, "task.create", json!({"title": "T1"})).await;
    call(&mut ws_a, "task.create", json!({"title": "T2"})).await;

    call(&mut ws_a, "task.get_next", json!({})).await;
    call(&mut ws_b, "task.get_next", json!({})).await;

    let res = call(&mut ws_a, "task.list", json!({"assigned_agent_id": "agent-x15a"})).await;
    let tasks = res.result.unwrap();
    let tasks = tasks.as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["assigned_agent_id"], "agent-x15a");
}

// ── topic.get ─────────────────────────────────────────────────────────────────

// AT-X16: topic.get returns topic with comments
#[tokio::test]
async fn topic_get_returns_topic_and_comments() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x16").await;

    let res = call(&mut ws, "topic.create", json!({"title": "With Comments", "content": "body"})).await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "topic.comment", json!({"topic_id": &topic_id, "content": "Reply 1"})).await;
    call(&mut ws, "topic.comment", json!({"topic_id": &topic_id, "content": "Reply 2"})).await;

    let res = call(&mut ws, "topic.get", json!({"id": &topic_id})).await;
    assert!(res.error.is_none());
    let r = res.result.unwrap();
    assert_eq!(r["topic"]["title"], "With Comments");
    let comments = r["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0]["content"], "Reply 1");
    assert_eq!(comments[1]["content"], "Reply 2");
}

// ── agent.register ────────────────────────────────────────────────────────────

// AT-X17: agent.register with empty id returns error
#[tokio::test]
async fn agent_register_empty_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x17").await;

    let res = call(&mut ws, "agent.register", json!({"id": "", "name": "n"})).await;
    assert!(res.error.is_some(), "empty agent id should be an error");
}

// AT-X18: agent.list returns registered agents
#[tokio::test]
async fn agent_list_shows_registered_agents() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x18").await;

    call(&mut ws, "agent.register", json!({"id": "agent-x18", "name": "X18 Agent"})).await;

    let res = call(&mut ws, "agent.list", json!({})).await;
    assert!(res.error.is_none());
    let agents = res.result.unwrap();
    let agents = agents.as_array().unwrap();
    let found = agents.iter().any(|a| a["id"] == "agent-x18");
    assert!(found, "registered agent should appear in agent.list");
}

// ── push.list empty ───────────────────────────────────────────────────────────

// AT-X19: push.list returns empty array when no messages
#[tokio::test]
async fn push_list_empty_when_no_messages() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x19").await;

    let res = call(&mut ws, "push.list", json!({})).await;
    assert!(res.error.is_none());
    let msgs = res.result.unwrap();
    assert!(msgs.as_array().unwrap().is_empty());
}

// ── task ordering / position ──────────────────────────────────────────────────

// AT-X20: tasks are dispatched in dependency-respecting order
#[tokio::test]
async fn tasks_dispatched_in_dependency_order() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-x20").await;

    let a = call(&mut ws, "task.create", json!({"title": "Step A"})).await.result.unwrap();
    let b = call(&mut ws, "task.create", json!({"title": "Step B"})).await.result.unwrap();
    let a_id = a["id"].as_str().unwrap();
    let b_id = b["id"].as_str().unwrap();

    call(&mut ws, "task.set_dependency", json!({"task_id": b_id, "depends_on_id": a_id})).await;

    // Must get A first.
    let first = call(&mut ws, "task.get_next", json!({})).await.result.unwrap();
    assert_eq!(first["id"].as_str().unwrap(), a_id, "should get A first");

    // Complete A — the complete response auto-claims the next available task (B).
    let complete_res = call(&mut ws, "task.complete", json!({"id": a_id})).await;
    assert!(complete_res.error.is_none(), "complete failed: {:?}", complete_res.error);
    let r = complete_res.result.unwrap();
    let next = &r["next_task"];
    assert!(!next.is_null(), "next_task should be B after A is completed");
    assert_eq!(next["id"].as_str().unwrap(), b_id, "next_task should be B");
}
