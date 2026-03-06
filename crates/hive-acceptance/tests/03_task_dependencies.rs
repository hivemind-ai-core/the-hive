//! Acceptance tests: task dependency ordering.
//!
//! Covers: task.set_dependency, topological sort, blocking behaviour.

use hive_acceptance::*;

// AT-10: task.get_next skips tasks whose dependencies are not yet done
#[tokio::test]
async fn dep_blocks_task_from_being_claimed() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-10").await;

    let id_a = call(&mut ws, "task.create", json!({"title": "Task A"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();
    let id_b = call(&mut ws, "task.create", json!({"title": "Task B"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    // B depends on A
    let res = call(&mut ws, "task.set_dependency", json!({
        "task_id": &id_b,
        "depends_on_id": &id_a
    })).await;
    assert!(res.error.is_none());

    // get_next should claim A (B is blocked by unfinished dep)
    let res = call(&mut ws, "task.get_next", json!({})).await;
    let claimed = res.result.unwrap();
    assert!(!claimed.is_null());
    assert_eq!(claimed["id"].as_str().unwrap(), id_a.as_str());

    // get_next again → null (A in-progress, B blocked by A not done)
    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert!(res.result.is_none(), "expected null, got: {:?}", res.result);
}

// AT-11: task.get_next returns blocked task after dependency is completed
#[tokio::test]
async fn dep_unblocks_when_satisfied() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-11").await;

    let id_a = call(&mut ws, "task.create", json!({"title": "Dep A"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();
    let id_b = call(&mut ws, "task.create", json!({"title": "Dep B"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    call(&mut ws, "task.set_dependency", json!({
        "task_id": &id_b,
        "depends_on_id": &id_a
    })).await;

    // Claim A
    let res = call(&mut ws, "task.get_next", json!({})).await;
    assert_eq!(res.result.unwrap()["id"].as_str().unwrap(), id_a.as_str());

    // Complete A — response should include B as next_task
    let res = call(&mut ws, "task.complete", json!({"id": &id_a})).await;
    assert!(res.error.is_none(), "complete failed: {:?}", res.error);
    let r = res.result.unwrap();
    let next = &r["next_task"];
    assert!(!next.is_null(), "B should be available after A is done");
    assert_eq!(next["id"].as_str().unwrap(), id_b.as_str());
    assert_eq!(next["status"], "in-progress");
}

// AT-12: task.set_dependency triggers topological re-sort of positions
#[tokio::test]
async fn set_dependency_reorders_positions() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-12").await;

    // Create child first, then parent (wrong order intentionally)
    let id_child = call(&mut ws, "task.create", json!({"title": "Child Task"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();
    let id_parent = call(&mut ws, "task.create", json!({"title": "Parent Task"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    // Child depends on parent → parent must come first in topological order
    let res = call(&mut ws, "task.set_dependency", json!({
        "task_id": &id_child,
        "depends_on_id": &id_parent
    })).await;
    assert!(res.error.is_none());

    // task.list returns tasks in position order; parent should precede child
    let res = call(&mut ws, "task.list", json!({})).await;
    let tasks = res.result.unwrap();
    let tasks = tasks.as_array().unwrap();
    let parent_pos = tasks.iter().position(|t| t["id"] == id_parent.as_str()).unwrap();
    let child_pos = tasks.iter().position(|t| t["id"] == id_child.as_str()).unwrap();
    assert!(parent_pos < child_pos, "parent should precede child after topo reorder");
}

// AT-13: In-progress task set as dependency of itself is rejected (cycle)
#[tokio::test]
async fn dep_cycle_is_rejected() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-13").await;

    let id_a = call(&mut ws, "task.create", json!({"title": "Task A"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();
    let id_b = call(&mut ws, "task.create", json!({"title": "Task B"}))
        .await.result.unwrap()["id"].as_str().unwrap().to_string();

    // A depends on B (valid)
    let res = call(&mut ws, "task.set_dependency", json!({
        "task_id": &id_a,
        "depends_on_id": &id_b
    })).await;
    assert!(res.error.is_none(), "first dep should succeed");

    // B depends on A → cycle, should be rejected
    let res = call(&mut ws, "task.set_dependency", json!({
        "task_id": &id_b,
        "depends_on_id": &id_a
    })).await;
    assert!(res.error.is_some(), "cycle should be rejected with an error");
}
