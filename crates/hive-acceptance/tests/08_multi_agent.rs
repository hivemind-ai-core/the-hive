//! Acceptance tests: multi-agent coordination scenarios.
//!
//! Covers: two agents claiming distinct tasks, cross-agent push messages,
//! cross-agent topic wait.

use hive_acceptance::*;

// AT-28: Two agents each claim a different task; no double-claiming
#[tokio::test]
async fn two_agents_claim_distinct_tasks() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-28a").await;
    let mut ws_b = connect(addr, "agent-28b").await;

    // Create two tasks
    let id1 = call(&mut ws_a, "task.create", json!({"title": "Task One"}))
        .await
        .result
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id2 = call(&mut ws_a, "task.create", json!({"title": "Task Two"}))
        .await
        .result
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Both agents claim (sequentially; get_next is serialized via DB mutex)
    let claim_a = call(&mut ws_a, "task.get_next", json!({})).await;
    let claim_b = call(&mut ws_b, "task.get_next", json!({})).await;

    let task_a_id = claim_a.result.unwrap()["id"].as_str().unwrap().to_string();
    let task_b_id = claim_b.result.unwrap()["id"].as_str().unwrap().to_string();

    // Both got different tasks
    assert_ne!(task_a_id, task_b_id);

    // Together they cover both created tasks
    let claimed: std::collections::HashSet<&str> = [task_a_id.as_str(), task_b_id.as_str()]
        .into_iter()
        .collect();
    assert!(claimed.contains(id1.as_str()));
    assert!(claimed.contains(id2.as_str()));
}

// AT-29: Agent A sends push to Agent B; B's push.list shows the message
#[tokio::test]
async fn cross_agent_push_delivery() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-29a").await;

    // A sends to B while B is disconnected → stored as undelivered
    let res = call(
        &mut ws_a,
        "push.send",
        json!({
            "to_agent_id": "agent-29b",
            "content": "Cross-agent message"
        }),
    )
    .await;
    assert!(res.error.is_none(), "push.send failed: {:?}", res.error);

    // B connects and retrieves
    let mut ws_b = connect(addr, "agent-29b").await;
    let res = call(&mut ws_b, "push.list", json!({})).await;
    assert!(res.error.is_none());
    let msgs = res.result.unwrap();
    let msgs = msgs.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"], "Cross-agent message");
    assert_eq!(msgs[0]["from_agent_id"], "agent-29a");
    assert_eq!(msgs[0]["to_agent_id"], "agent-29b");
}

// AT-30: Agent A posts topic comment; Agent B's topic.wait resolves
#[tokio::test]
async fn cross_agent_topic_wait() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-30a").await;
    let mut ws_b = connect(addr, "agent-30b").await;

    // Agent A creates a topic
    let res = call(
        &mut ws_a,
        "topic.create",
        json!({
            "title": "Shared Topic",
            "content": "Waiting for replies"
        }),
    )
    .await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Spawn Agent A to post a comment after a short delay (new WS to avoid borrow conflict)
    let addr_clone = addr;
    let topic_id_clone = topic_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let mut poster = connect(addr_clone, "agent-30a-poster").await;
        call(
            &mut poster,
            "topic.comment",
            json!({
                "topic_id": &topic_id_clone,
                "content": "Agent A replied!"
            }),
        )
        .await;
    });

    // Agent B waits for a new comment
    let res = call(
        &mut ws_b,
        "topic.wait",
        json!({
            "id": &topic_id,
            "since_count": 0,
            "timeout_secs": 5
        }),
    )
    .await;
    assert!(
        res.error.is_none(),
        "topic.wait should resolve: {:?}",
        res.error
    );
    let result = res.result.unwrap();
    let comments = result["comments"].as_array().unwrap();
    assert!(!comments.is_empty());
    assert_eq!(comments[0]["content"], "Agent A replied!");

    // Keep connections alive until end of test
    let _ = (ws_a, ws_b);
}
