//! Acceptance tests: error handling.
//!
//! Covers: missing params, unknown IDs, invalid status transitions, bad inputs.

use hive_acceptance::*;

// ── task.create ───────────────────────────────────────────────────────────────

// AT-E01: task.create with empty title returns an error
#[tokio::test]
async fn task_create_empty_title_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e01").await;

    let res = call(&mut ws, "task.create", json!({"title": ""})).await;
    assert!(res.error.is_some(), "empty title should be an error");
}

// AT-E02: task.create with whitespace-only title returns an error
#[tokio::test]
async fn task_create_whitespace_title_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e02").await;

    let res = call(&mut ws, "task.create", json!({"title": "   "})).await;
    assert!(res.error.is_some(), "whitespace-only title should be an error");
}

// AT-E03: task.create with missing title field returns an error
#[tokio::test]
async fn task_create_missing_title_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e03").await;

    let res = call(&mut ws, "task.create", json!({})).await;
    assert!(res.error.is_some(), "missing title should be an error");
}

// ── task.get ──────────────────────────────────────────────────────────────────

// AT-E04: task.get with unknown ID returns error
#[tokio::test]
async fn task_get_unknown_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e04").await;

    let res = call(&mut ws, "task.get", json!({"id": "does-not-exist"})).await;
    assert!(res.error.is_some(), "unknown task ID should be an error");
}

// AT-E05: task.get without id param returns error
#[tokio::test]
async fn task_get_missing_id_param_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e05").await;

    let res = call(&mut ws, "task.get", json!({})).await;
    assert!(res.error.is_some(), "missing id param should be an error");
}

// ── task.update ───────────────────────────────────────────────────────────────

// AT-E06: task.update with unknown ID returns error
#[tokio::test]
async fn task_update_unknown_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e06").await;

    let res = call(&mut ws, "task.update", json!({"id": "ghost-id", "description": "x"})).await;
    assert!(res.error.is_some(), "updating unknown task should be an error");
}

// AT-E07: task.update with invalid status transition errors
#[tokio::test]
async fn task_update_invalid_status_transition_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e07").await;

    let res = call(&mut ws, "task.create", json!({"title": "T"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // pending → done is not a valid direct transition.
    let res = call(&mut ws, "task.update", json!({"id": &id, "status": "done"})).await;
    assert!(res.error.is_some(), "pending → done should be invalid");
}

// AT-E08: task.update with unknown status value errors
#[tokio::test]
async fn task_update_unknown_status_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e08").await;

    let res = call(&mut ws, "task.create", json!({"title": "T"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.update", json!({"id": &id, "status": "flying"})).await;
    assert!(res.error.is_some(), "unknown status string should be an error");
}

// ── task.complete ─────────────────────────────────────────────────────────────

// AT-E09: task.complete with unknown ID returns error
#[tokio::test]
async fn task_complete_unknown_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e09").await;

    let res = call(&mut ws, "task.complete", json!({"id": "ghost"})).await;
    assert!(res.error.is_some(), "completing unknown task should be an error");
}

// AT-E10: task.complete without id param returns error
#[tokio::test]
async fn task_complete_missing_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e10").await;

    let res = call(&mut ws, "task.complete", json!({})).await;
    assert!(res.error.is_some(), "missing id param should be an error");
}

// ── task.set_dependency ───────────────────────────────────────────────────────

// AT-E11: task.set_dependency on self returns error
#[tokio::test]
async fn task_set_dependency_self_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e11").await;

    let res = call(&mut ws, "task.create", json!({"title": "T"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.set_dependency", json!({"task_id": &id, "depends_on_id": &id})).await;
    assert!(res.error.is_some(), "self-dependency should be an error");
}

// AT-E12: task.set_dependency with missing task_id errors
#[tokio::test]
async fn task_set_dependency_missing_task_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e12").await;

    let res = call(&mut ws, "task.create", json!({"title": "T"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.set_dependency", json!({"depends_on_id": &id})).await;
    assert!(res.error.is_some(), "missing task_id should be an error");
}

// AT-E13: circular dependency returns error
#[tokio::test]
async fn task_set_dependency_cycle_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e13").await;

    let a = call(&mut ws, "task.create", json!({"title": "A"})).await.result.unwrap();
    let b = call(&mut ws, "task.create", json!({"title": "B"})).await.result.unwrap();
    let a_id = a["id"].as_str().unwrap();
    let b_id = b["id"].as_str().unwrap();

    // A depends on B
    call(&mut ws, "task.set_dependency", json!({"task_id": a_id, "depends_on_id": b_id})).await;

    // B depends on A → cycle
    let res = call(&mut ws, "task.set_dependency", json!({"task_id": b_id, "depends_on_id": a_id})).await;
    assert!(res.error.is_some(), "circular dependency should be an error");
}

// ── task.split ────────────────────────────────────────────────────────────────

// AT-E14: task.split with missing id errors
#[tokio::test]
async fn task_split_missing_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e14").await;

    let res = call(&mut ws, "task.split", json!({"subtasks": ["S1", "S2"]})).await;
    assert!(res.error.is_some(), "missing id should be an error");
}

// AT-E15: task.split with missing subtasks errors
#[tokio::test]
async fn task_split_missing_subtasks_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e15").await;

    let res = call(&mut ws, "task.create", json!({"title": "T"})).await;
    let id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(&mut ws, "task.split", json!({"id": &id})).await;
    assert!(res.error.is_some(), "missing subtasks should be an error");
}

// AT-E16: task.split on nonexistent task errors
#[tokio::test]
async fn task_split_nonexistent_task_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e16").await;

    let res = call(&mut ws, "task.split", json!({"id": "ghost", "subtasks": ["S1"]})).await;
    assert!(res.error.is_some(), "splitting nonexistent task should be an error");
}

// ── topic.get ─────────────────────────────────────────────────────────────────

// AT-E17: topic.get with unknown ID returns error
#[tokio::test]
async fn topic_get_unknown_id_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e17").await;

    let res = call(&mut ws, "topic.get", json!({"id": "ghost-topic"})).await;
    assert!(res.error.is_some(), "unknown topic ID should be an error");
}

// AT-E18: topic.comment with unknown topic ID returns error
#[tokio::test]
async fn topic_comment_unknown_topic_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e18").await;

    let res = call(&mut ws, "topic.comment", json!({
        "topic_id": "ghost-topic-id",
        "content": "Hello"
    })).await;
    assert!(res.error.is_some(), "commenting on unknown topic should be an error");
}

// AT-E19: topic.create with missing title errors
#[tokio::test]
async fn topic_create_missing_title_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e19").await;

    let res = call(&mut ws, "topic.create", json!({"content": "body"})).await;
    assert!(res.error.is_some(), "missing title should be an error");
}

// AT-E20: topic.create with missing content errors
#[tokio::test]
async fn topic_create_missing_content_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e20").await;

    let res = call(&mut ws, "topic.create", json!({"title": "T"})).await;
    assert!(res.error.is_some(), "missing content should be an error");
}

// ── push messages ─────────────────────────────────────────────────────────────

// AT-E21: push.send with missing to_agent_id errors
#[tokio::test]
async fn push_send_missing_recipient_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e21").await;

    let res = call(&mut ws, "push.send", json!({"content": "hello"})).await;
    assert!(res.error.is_some(), "missing to_agent_id should be an error");
}

// AT-E22: push.send with missing content errors
#[tokio::test]
async fn push_send_missing_content_errors() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e22").await;

    let res = call(&mut ws, "push.send", json!({"to_agent_id": "agent-b"})).await;
    assert!(res.error.is_some(), "missing content should be an error");
}

// AT-E23: push.ack with unknown message ID is graceful (no error or ok)
#[tokio::test]
async fn push_ack_unknown_message_id_is_ok() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e23").await;

    // ACKing a nonexistent message should not crash the server.
    let res = call(&mut ws, "push.ack", json!({"id": "nonexistent-msg-id"})).await;
    // It may succeed silently or return an error, but must not crash.
    // We just verify the server responds.
    let _ = res;
}

// ── unknown method ────────────────────────────────────────────────────────────

// AT-E24: calling unknown method returns error
#[tokio::test]
async fn unknown_method_returns_error() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-e24").await;

    let res = call(&mut ws, "totally.unknown.method", json!({})).await;
    assert!(res.error.is_some(), "unknown method should return an error");
}
