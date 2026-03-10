//! Acceptance tests: task CRUD lifecycle.
//!
//! Covers: task.create, task.get, task.list, task.update, task.complete

use hive_acceptance::*;

// AT-01: Create a task and retrieve it by ID
#[tokio::test]
async fn task_create_and_get() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-01").await;

    let res = call(&mut ws, "task.create", json!({"title": "My Task"})).await;
    assert!(res.error.is_none(), "create failed: {:?}", res.error);
    let task = res.result.unwrap();
    let id = task["id"].as_str().expect("id missing").to_string();
    assert_eq!(task["title"], "My Task");
    assert_eq!(task["status"], "pending");
    assert!(task["created_at"].is_string());

    let res = call(&mut ws, "task.get", json!({"id": &id})).await;
    assert!(res.error.is_none(), "get failed: {:?}", res.error);
    let got = res.result.unwrap();
    assert_eq!(got["id"].as_str().unwrap(), id.as_str());
    assert_eq!(got["title"], "My Task");
    assert_eq!(got["status"], "pending");
    assert!(got["created_at"].is_string());
}

// AT-02: List tasks with status filter
#[tokio::test]
async fn task_list_by_status() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-02").await;

    call(&mut ws, "task.create", json!({"title": "Task Alpha"})).await;
    call(&mut ws, "task.create", json!({"title": "Task Beta"})).await;

    // Claim first available task (makes it in-progress)
    let _ = call(&mut ws, "task.get_next", json!({})).await;

    // Pending: only one task remains
    let res = call(&mut ws, "task.list", json!({"status": "pending"})).await;
    assert!(res.error.is_none());
    let pending = res.result.unwrap();
    let pending = pending.as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending.iter().all(|t| t["status"] == "pending"));

    // In-progress: the claimed one
    let res = call(&mut ws, "task.list", json!({"status": "in-progress"})).await;
    assert!(res.error.is_none());
    let in_prog = res.result.unwrap();
    let in_prog = in_prog.as_array().unwrap();
    assert_eq!(in_prog.len(), 1);
    assert_eq!(in_prog[0]["status"], "in-progress");
}

// AT-03: List tasks with tag filter
#[tokio::test]
async fn task_list_by_tag() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-03").await;

    call(
        &mut ws,
        "task.create",
        json!({"title": "Rust Task", "tags": ["rust", "backend"]}),
    )
    .await;
    call(
        &mut ws,
        "task.create",
        json!({"title": "Python Task", "tags": ["python"]}),
    )
    .await;
    call(&mut ws, "task.create", json!({"title": "No Tag Task"})).await;

    let res = call(&mut ws, "task.list", json!({"tag": "rust"})).await;
    assert!(res.error.is_none());
    let tasks = res.result.unwrap();
    let tasks = tasks.as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["title"], "Rust Task");
}

// AT-04: Update task description and tags
#[tokio::test]
async fn task_update_fields() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-04").await;

    let res = call(&mut ws, "task.create", json!({"title": "Original Title"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(
        &mut ws,
        "task.update",
        json!({
            "id": &id,
            "description": "New description",
            "tags": ["rust", "test"]
        }),
    )
    .await;
    assert!(res.error.is_none(), "update failed: {:?}", res.error);
    let updated = res.result.unwrap();
    assert_eq!(updated["description"], "New description");

    // Confirm via get
    let res = call(&mut ws, "task.get", json!({"id": &id})).await;
    let got = res.result.unwrap();
    assert_eq!(got["description"], "New description");
    let tags: Vec<&str> = got["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(tags.contains(&"rust"));
    assert!(tags.contains(&"test"));
}

// AT-05: Complete a task; response includes completed ID and next_task field
#[tokio::test]
async fn task_complete_returns_next() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-05").await;

    call(&mut ws, "task.create", json!({"title": "First"})).await;
    call(&mut ws, "task.create", json!({"title": "Second"})).await;

    // Claim first
    let claim = call(&mut ws, "task.get_next", json!({})).await;
    let task1_id = claim.result.unwrap()["id"].as_str().unwrap().to_string();

    // Complete first
    let res = call(&mut ws, "task.complete", json!({"id": &task1_id})).await;
    assert!(res.error.is_none(), "complete failed: {:?}", res.error);
    let r = res.result.unwrap();
    assert_eq!(r["completed"].as_str().unwrap(), task1_id.as_str());
    let next = &r["next_task"];
    assert!(!next.is_null(), "next_task should not be null");
    assert_eq!(next["title"], "Second");
    assert_eq!(next["status"], "in-progress");
}
