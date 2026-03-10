//! Handlers for topic.* WS methods.

use anyhow::Result;
use hive_core::types::{PushMessage, Topic};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent_registry::{self, AgentRegistry},
    communication as db_comm,
    db::DbPool,
    message_board as db_mb,
};

/// Default value for the `since` timestamp filter in `topic.list_new`.
/// 0 means "return all topics" (since Unix epoch).
const DEFAULT_SINCE_TIMESTAMP: i64 = 0;

/// Default value for the `since_count` parameter in topic.wait.
/// 0 means "wait for any new comment" regardless of existing count.
const DEFAULT_SINCE_COUNT: u64 = 0;

/// Default timeout for topic.wait when the caller does not supply `timeout_secs`.
/// 30 seconds is a reasonable upper bound for a long-poll request.
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 30;

#[derive(Deserialize)]
struct CreateParams {
    title: String,
    content: String,
    creator_agent_id: Option<String>,
}

pub fn create(pool: &DbPool, agent_id: &str, params: Option<Value>) -> Result<Value> {
    let p: CreateParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.title.trim().is_empty() {
        anyhow::bail!("title must not be empty");
    }
    // Use the connection's agent_id as creator (server-side enforcement).
    let creator = if agent_id.is_empty() { p.creator_agent_id } else { Some(agent_id.to_string()) };
    let topic = Topic::new(p.title, p.content, creator);
    db_mb::insert_topic(pool, &topic)?;
    Ok(serde_json::to_value(&topic)?)
}

pub fn list(pool: &DbPool, _params: Option<Value>) -> Result<Value> {
    let topics = db_mb::list_topics(pool)?;
    Ok(serde_json::to_value(&topics)?)
}

#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
pub fn list_new(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let since = params
        .as_ref()
        .and_then(|v| v.get("since"))
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_SINCE_TIMESTAMP);
    let topics = db_mb::list_topics_since(pool, since)?;
    Ok(serde_json::to_value(&topics)?)
}

pub fn comment(pool: &DbPool, registry: &AgentRegistry, agent_id: &str, params: Option<Value>) -> Result<Value> {
    use hive_core::types::Comment;
    #[derive(serde::Deserialize)]
    struct CommentParams {
        topic_id: String,
        content: String,
        creator_agent_id: Option<String>,
    }
    let p: CommentParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    // Use the connection's agent_id as creator (server-side enforcement).
    let creator = if agent_id.is_empty() { p.creator_agent_id } else { Some(agent_id.to_string()) };
    let comment = Comment::new(p.topic_id, p.content, creator);
    db_mb::insert_comment(pool, &comment)?;

    // Send @mention notifications.
    let commenter = comment.creator_agent_id.as_deref().unwrap_or("unknown");
    for mentioned_id in extract_mentions(&comment.content) {
        // Skip self-mentions.
        if comment.creator_agent_id.as_deref() == Some(&*mentioned_id) {
            continue;
        }
        let notif = PushMessage::new(
            mentioned_id.clone(),
            format!(
                "[notification] You have been tagged in topic #{} by agent {}",
                comment.topic_id, commenter
            ),
            comment.creator_agent_id.clone(),
        );
        // Best-effort: the comment still succeeds even if notification storage fails.
        if let Err(e) = db_comm::insert_message(pool, &notif) {
            tracing::warn!(mentioned_agent = %mentioned_id, topic_id = %comment.topic_id, error = %e, "@mention notification DB insert failed");
        }
        if let Ok(val) = serde_json::to_value(std::slice::from_ref(&notif)) {
            agent_registry::notify_agent(registry, &mentioned_id, &val);
        }
    }

    Ok(serde_json::to_value(&comment)?)
}

/// Extract `@agent-id` mentions from comment content.
/// Strips trailing punctuation so `@agent,` and `@agent.` work correctly.
fn extract_mentions(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|word| {
            let id = word
                .strip_prefix('@')?
                .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if id.is_empty() { None } else { Some(id.to_string()) }
        })
        .collect()
}

/// Long-poll until the comment count for a topic exceeds `since_count`.
///
/// Polls the DB once per second. Returns the full topic and its comments as soon
/// as `comments.len() > since_count`. If no new comments arrive within
/// `timeout_secs` (default 30 s) the call returns an error.
///
/// Params: `{ "id": "<topic-id>", "since_count": <u64>, "timeout_secs": <u64> }`
pub async fn wait(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let id = p.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?
        .to_string();
    let since_count = p.get("since_count").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_SINCE_COUNT) as usize;
    let timeout_secs = p.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_WAIT_TIMEOUT_SECS);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let comments = db_mb::get_comments(pool, &id)?;
        if comments.len() > since_count {
            let topic = db_mb::get_topic(pool, &id)?
                .ok_or_else(|| anyhow::anyhow!("topic not found"))?;
            return Ok(serde_json::json!({ "topic": topic, "comments": comments }));
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for new comments");
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
pub fn get(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let id = params
        .as_ref()
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;

    let topic = db_mb::get_topic(pool, id)?
        .ok_or_else(|| anyhow::anyhow!("topic not found"))?;
    let comments = db_mb::get_comments(pool, id)?;

    Ok(serde_json::json!({
        "topic": topic,
        "comments": comments,
    }))
}
