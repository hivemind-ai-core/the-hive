//! Acceptance tests: task splitting.
//!
//! Covers: task.split cancels original, creates ordered subtasks.

use hive_acceptance::*;

// AT-14: task.split cancels the original task
#[tokio::test]
async fn split_cancels_original() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-14").await;

    let id = call(&mut ws, "task.create", json!({"title": "To Split"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.split", json!({
        "id": &id,
        "subtasks": ["Sub One", "Sub Two"]
    })).await;
    assert!(res.error.is_none(), "split failed: {:?}", res.error);

    // Original should be cancelled
    let res = call(&mut ws, "task.get", json!({"id": &id})).await;
    assert!(res.error.is_none());
    let original = res.result.unwrap();
    assert_eq!(original["status"], "cancelled");
}

// AT-15: task.split creates subtasks in the given order
#[tokio::test]
async fn split_creates_ordered_subtasks() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-15").await;

    let id = call(&mut ws, "task.create", json!({"title": "To Split"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.split", json!({
        "id": &id,
        "subtasks": ["First Sub", "Second Sub", "Third Sub"]
    })).await;
    assert!(res.error.is_none(), "split failed: {:?}", res.error);
    let subtasks = res.result.unwrap();
    let subtasks = subtasks.as_array().unwrap();

    assert_eq!(subtasks.len(), 3);
    assert_eq!(subtasks[0]["title"], "First Sub");
    assert_eq!(subtasks[1]["title"], "Second Sub");
    assert_eq!(subtasks[2]["title"], "Third Sub");
    // All should be pending initially
    assert!(subtasks.iter().all(|t| t["status"] == "pending"));
}

// AT-16: Subtasks are claimable via task.get_next after splitting
#[tokio::test]
async fn split_subtasks_are_claimable() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-16").await;

    let id = call(&mut ws, "task.create", json!({"title": "To Split"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.split", json!({
        "id": &id,
        "subtasks": ["Sub One", "Sub Two"]
    })).await;
    assert!(res.error.is_none());
    let subtasks = res.result.unwrap();
    let sub1_id = subtasks[0]["id"].as_str().unwrap().to_string();

    // Only the first subtask has no deps and is claimable
    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert!(res.error.is_none());
    let claimed = res.result.unwrap();
    assert!(!claimed.is_null(), "first subtask should be claimable");
    assert_eq!(claimed["id"].as_str().unwrap(), sub1_id.as_str());
    assert_eq!(claimed["status"], "in-progress");
}
