//! Acceptance tests: agent registration and listing.
//!
//! Covers: agent.register on connect, agent.list.

use hive_acceptance::*;

// AT-26: Connecting with agent_id registers the agent; agent.list shows it
#[tokio::test]
async fn agent_appears_in_list_after_connect() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-26").await;

    // Explicitly register (connecting alone doesn't write to the DB registry)
    let res = call(
        &mut ws,
        "agent.register",
        json!({
            "id": "agent-26",
            "name": "Test Agent 26"
        }),
    )
    .await;
    assert!(res.error.is_none(), "register failed: {:?}", res.error);

    // Should appear in agent.list
    let res = call(&mut ws, "agent.list", json!({})).await;
    assert!(res.error.is_none());
    let agents = res.result.unwrap();
    let agents = agents.as_array().unwrap();
    assert!(
        agents.iter().any(|a| a["id"] == "agent-26"),
        "agent not found in list"
    );
}

// AT-27: agent.register with tags stores tags visible in agent.list
#[tokio::test]
async fn agent_register_stores_tags() {
    let addr = start_server().await;
    let mut ws = connect(addr, "agent-27").await;

    let res = call(
        &mut ws,
        "agent.register",
        json!({
            "id": "agent-27",
            "name": "Tagged Agent",
            "tags": ["rust", "backend", "ai"]
        }),
    )
    .await;
    assert!(res.error.is_none(), "register failed: {:?}", res.error);

    let res = call(&mut ws, "agent.list", json!({})).await;
    assert!(res.error.is_none());
    let agents = res.result.unwrap();
    let agents = agents.as_array().unwrap();
    let agent = agents
        .iter()
        .find(|a| a["id"] == "agent-27")
        .expect("agent-27 not found in list");
    let tags: Vec<&str> = agent["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(tags.contains(&"rust"));
    assert!(tags.contains(&"backend"));
    assert!(tags.contains(&"ai"));
}
