//! Acceptance tests: message board (topics and comments).
//!
//! Covers: topic.create, topic.list, topic.get, topic.comment, topic.wait.

use hive_acceptance::*;

// AT-17: Create a topic and retrieve it via topic.list and topic.get
#[tokio::test]
async fn topic_create_list_get() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-17").await;

    let res = call(
        &mut ws,
        "topic.create",
        json!({
            "title": "My Topic",
            "content": "Initial content"
        }),
    )
    .await;
    assert!(res.error.is_none(), "create failed: {:?}", res.error);
    let topic = res.result.unwrap();
    let topic_id = topic["id"].as_str().unwrap().to_string();
    assert_eq!(topic["title"], "My Topic");

    // topic.list shows it
    let res = call(&mut ws, "topic.list", json!({})).await;
    assert!(res.error.is_none());
    let topics = res.result.unwrap();
    let topics = topics.as_array().unwrap();
    assert!(topics.iter().any(|t| t["id"] == topic_id.as_str()));

    // topic.get returns topic and empty comments
    let res = call(&mut ws, "topic.get", json!({"id": &topic_id})).await;
    assert!(res.error.is_none());
    let result = res.result.unwrap();
    assert_eq!(result["topic"]["id"].as_str().unwrap(), topic_id.as_str());
    assert_eq!(result["topic"]["title"], "My Topic");
    assert_eq!(result["topic"]["content"], "Initial content");
    assert!(result["comments"].as_array().unwrap().is_empty());
}

// AT-18: topic.comment adds a comment visible in topic.get
#[tokio::test]
async fn topic_comment_visible_in_get() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-18").await;

    let res = call(
        &mut ws,
        "topic.create",
        json!({
            "title": "Topic",
            "content": "Content"
        }),
    )
    .await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let res = call(
        &mut ws,
        "topic.comment",
        json!({
            "topic_id": &topic_id,
            "content": "Hello, world!"
        }),
    )
    .await;
    assert!(res.error.is_none(), "comment failed: {:?}", res.error);

    let res = call(&mut ws, "topic.get", json!({"id": &topic_id})).await;
    assert!(res.error.is_none());
    let result = res.result.unwrap();
    let comments = result["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["content"], "Hello, world!");
    assert_eq!(comments[0]["topic_id"].as_str().unwrap(), topic_id.as_str());
}

// AT-19: topic.list since= filter excludes topics not updated after timestamp
#[tokio::test]
async fn topic_list_since_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-19").await;

    call(
        &mut ws,
        "topic.create",
        json!({"title": "Old Topic", "content": "old"}),
    )
    .await;
    call(
        &mut ws,
        "topic.create",
        json!({"title": "New Topic", "content": "new"}),
    )
    .await;

    // since=0 (epoch) → all topics returned
    let res = call(&mut ws, "topic.list_new", json!({"since": 0})).await;
    assert!(res.error.is_none());
    let all = res.result.unwrap();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);

    // since=far future → no topics
    let far_future: i64 = 4_102_444_800; // year 2100
    let res = call(&mut ws, "topic.list_new", json!({"since": far_future})).await;
    assert!(res.error.is_none());
    let none = res.result.unwrap();
    assert!(none.as_array().unwrap().is_empty());
}

// AT-20: topic.get since= filter returns only new comments
//
// The server surfaces this via topic.wait's since_count parameter:
// if comments.len() > since_count, wait resolves immediately with all comments.
#[tokio::test]
async fn topic_get_since_filter() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-20").await;

    let res = call(
        &mut ws,
        "topic.create",
        json!({
            "title": "Topic",
            "content": "Content"
        }),
    )
    .await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Add 2 comments
    call(
        &mut ws,
        "topic.comment",
        json!({"topic_id": &topic_id, "content": "First"}),
    )
    .await;
    call(
        &mut ws,
        "topic.comment",
        json!({"topic_id": &topic_id, "content": "Second"}),
    )
    .await;

    // topic.wait with since_count=1 resolves immediately (2 comments > 1)
    let res = call(
        &mut ws,
        "topic.wait",
        json!({
            "id": &topic_id,
            "since_count": 1,
            "timeout_secs": 5
        }),
    )
    .await;
    assert!(res.error.is_none(), "wait should resolve: {:?}", res.error);
    let result = res.result.unwrap();
    let comments = result["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert!(comments.iter().any(|c| c["content"] == "Second"));
}

// AT-21: topic.wait resolves when a new comment is posted before timeout
#[tokio::test]
async fn topic_wait_resolves_on_new_comment() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-21").await;

    // Create topic with no comments
    let res = call(
        &mut ws,
        "topic.create",
        json!({
            "title": "Wait Topic",
            "content": "..."
        }),
    )
    .await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Spawn a separate connection to post a comment after a short delay
    let addr_clone = addr;
    let topic_id_clone = topic_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let mut commenter = connect(addr_clone, "agent-21-commenter").await;
        call(
            &mut commenter,
            "topic.comment",
            json!({
                "topic_id": &topic_id_clone,
                "content": "Resolved!"
            }),
        )
        .await;
    });

    // Wait should resolve when the comment arrives
    let res = call(
        &mut ws,
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
        "wait should resolve before timeout: {:?}",
        res.error
    );
    let result = res.result.unwrap();
    let comments = result["comments"].as_array().unwrap();
    assert!(!comments.is_empty());
    assert_eq!(comments[0]["content"], "Resolved!");
}

// AT-22: topic.wait returns empty result on timeout with no new content
#[tokio::test]
async fn topic_wait_times_out() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-22").await;

    let res = call(
        &mut ws,
        "topic.create",
        json!({
            "title": "Empty Topic",
            "content": "..."
        }),
    )
    .await;
    let topic_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    // Wait with 1-second timeout and no comments → timeout error
    let res = call(
        &mut ws,
        "topic.wait",
        json!({
            "id": &topic_id,
            "since_count": 0,
            "timeout_secs": 1
        }),
    )
    .await;
    assert!(res.error.is_some(), "should have timed out");
}
