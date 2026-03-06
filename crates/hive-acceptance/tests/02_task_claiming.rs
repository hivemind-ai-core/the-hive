//! Acceptance tests: task claiming via task.get_next.
//!
//! Covers: task.get_next assignment behaviour, status transitions, null return.

use hive_acceptance::*;

// AT-06: task.get_next assigns task to calling agent and sets status in-progress
#[tokio::test]
async fn get_next_assigns_and_sets_in_progress() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-06").await;

    call(&mut ws, "task.create", json!({"title": "Claimable Task"})).await;

    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert!(res.error.is_none(), "get_next failed: {:?}", res.error);
    let task = res.result.unwrap();
    assert!(!task.is_null());
    assert_eq!(task["status"], "in-progress");
    assert_eq!(task["assigned_agent_id"], "agent-06");
    assert_eq!(task["title"], "Claimable Task");
}

// AT-07: task.get_next returns null when no unclaimed tasks are available
#[tokio::test]
async fn get_next_returns_null_when_empty() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-07").await;

    // No tasks exist — result is JSON null, normalized to None
    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert!(res.error.is_none());
    assert!(res.result.is_none(), "expected null result, got: {:?}", res.result);
}

// AT-08: task.get_next with tag filter only returns matching tasks
#[tokio::test]
async fn get_next_respects_tag_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-08").await;

    call(&mut ws, "task.create", json!({"title": "Rust Task", "tags": ["rust"]})).await;
    call(&mut ws, "task.create", json!({"title": "Python Task", "tags": ["python"]})).await;

    let res = call(&mut ws, "task.get_next", json!({"tag": "python"})).await;
    assert!(res.error.is_none());
    let task = res.result.unwrap();
    assert!(!task.is_null());
    assert_eq!(task["title"], "Python Task");
    assert_eq!(task["status"], "in-progress");

    // Rust task is still pending
    let res = call(&mut ws, "task.list", json!({"status": "pending"})).await;
    let pending = res.result.unwrap();
    let pending = pending.as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["title"], "Rust Task");
}

// AT-09: Already in-progress task is not returned by task.get_next to another agent
#[tokio::test]
async fn get_next_skips_claimed_tasks() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-09a").await;
    let mut ws_b = connect(addr, "agent-09b").await;

    // Only one task
    call(&mut ws_a, "task.create", json!({"title": "Only Task"})).await;

    // Agent A claims it
    let res_a = call(&mut ws_a, "task.get_next", json!({})).await;
    assert_eq!(res_a.result.unwrap()["status"], "in-progress");

    // Agent B gets nothing — result is JSON null, normalized to None
    let res_b = call(&mut ws_b, "task.get_next", json!({})).await;
    assert!(res_b.error.is_none());
    assert!(res_b.result.is_none(), "expected null result, got: {:?}", res_b.result);
}
