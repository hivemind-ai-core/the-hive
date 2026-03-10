//! Acceptance tests: push messages.
//!
//! Covers: push.send, push.list, push.ack delivery semantics.
//!
//! Note: when the recipient agent is connected at send time, the server performs
//! live delivery and marks the message as delivered immediately. Tests here keep
//! the recipient disconnected while the sender sends so that the message is stored
//! as undelivered, then connect the recipient and call push.list.

use hive_acceptance::*;

// AT-23: push.send delivers a message visible via push.list on the recipient
#[tokio::test]
async fn push_send_visible_in_list() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-23a").await;

    // A sends to B while B is not connected → stored undelivered
    let res = call(
        &mut ws_a,
        "push.send",
        json!({
            "to_agent_id": "agent-23b",
            "content": "Hello B!"
        }),
    )
    .await;
    assert!(res.error.is_none(), "push.send failed: {:?}", res.error);

    // B connects and lists messages
    let mut ws_b = connect(addr, "agent-23b").await;
    let res = call(&mut ws_b, "push.list", json!({})).await;
    assert!(res.error.is_none());
    let msgs = res.result.unwrap();
    let msgs = msgs.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"], "Hello B!");
    assert_eq!(msgs[0]["to_agent_id"], "agent-23b");
}

// AT-24: push.ack marks message as delivered; it no longer appears in push.list
#[tokio::test]
async fn push_ack_removes_from_list() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-24a").await;

    let res = call(
        &mut ws_a,
        "push.send",
        json!({
            "to_agent_id": "agent-24b",
            "content": "Ack me!"
        }),
    )
    .await;
    assert!(res.error.is_none());
    let msg_id = res.result.unwrap()["id"].as_str().unwrap().to_string();

    let mut ws_b = connect(addr, "agent-24b").await;

    // B sees the message
    let res = call(&mut ws_b, "push.list", json!({})).await;
    let msgs = res.result.unwrap();
    assert_eq!(msgs.as_array().unwrap().len(), 1);

    // B acks it
    let res = call(&mut ws_b, "push.ack", json!({"message_ids": [&msg_id]})).await;
    assert!(res.error.is_none(), "push.ack failed: {:?}", res.error);

    // Push list is now empty
    let res = call(&mut ws_b, "push.list", json!({})).await;
    let msgs = res.result.unwrap();
    assert!(msgs.as_array().unwrap().is_empty());
}

// AT-25: push.list only returns messages addressed to the calling agent
#[tokio::test]
async fn push_list_scoped_to_recipient() {
    let addr = start_server().await;
    let mut ws_a = connect(addr, "agent-25a").await;

    // A sends to B (B not connected)
    call(
        &mut ws_a,
        "push.send",
        json!({
            "to_agent_id": "agent-25b",
            "content": "For B only"
        }),
    )
    .await;

    // C connects and checks — should see nothing
    let mut ws_c = connect(addr, "agent-25c").await;
    let res = call(&mut ws_c, "push.list", json!({})).await;
    assert!(res.result.unwrap().as_array().unwrap().is_empty());

    // B connects and checks — should see the message
    let mut ws_b = connect(addr, "agent-25b").await;
    let res = call(&mut ws_b, "push.list", json!({})).await;
    let msgs = res.result.unwrap();
    let msgs = msgs.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"], "For B only");
    assert_eq!(msgs[0]["to_agent_id"], "agent-25b");
}
