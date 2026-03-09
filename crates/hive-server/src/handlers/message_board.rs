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

#[derive(Deserialize)]
struct CreateParams {
    title: String,
    content: String,
    creator_agent_id: Option<String>,
}

pub fn create(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p: CreateParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.title.trim().is_empty() {
        anyhow::bail!("title must not be empty");
    }
    let topic = Topic::new(p.title, p.content, p.creator_agent_id);
    db_mb::insert_topic(pool, &topic)?;
    Ok(serde_json::to_value(&topic)?)
}

pub fn list(pool: &DbPool, _params: Option<Value>) -> Result<Value> {
    let topics = db_mb::list_topics(pool)?;
    Ok(serde_json::to_value(&topics)?)
}

pub fn list_new(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let since = params
        .as_ref()
        .and_then(|v| v.get("since"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let topics = db_mb::list_topics_since(pool, since)?;
    Ok(serde_json::to_value(&topics)?)
}

pub fn comment(pool: &DbPool, registry: &AgentRegistry, params: Option<Value>) -> Result<Value> {
    use hive_core::types::Comment;
    #[derive(serde::Deserialize)]
    struct CommentParams {
        topic_id: String,
        content: String,
        creator_agent_id: Option<String>,
    }
    let p: CommentParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    let comment = Comment::new(p.topic_id, p.content, p.creator_agent_id);
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
        // Best-effort: ignore DB errors so the comment still succeeds.
        let _ = db_comm::insert_message(pool, &notif);
        if let Ok(val) = serde_json::to_value(std::slice::from_ref(&notif)) {
            agent_registry::notify_agent(registry, &mentioned_id, val);
        }
    }

    Ok(serde_json::to_value(&comment)?)
}

/// Extract `@agent-id` mentions from comment content.
/// Strips trailing punctuation so `@agent,` and `@agent.` work correctly.
fn extract_mentions(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in content.split_whitespace() {
        if let Some(id) = word.strip_prefix('@') {
            let id = id.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if !id.is_empty() {
                out.push(id.to_string());
            }
        }
    }
    out
}

/// Wait until the comment count for a topic exceeds `since_count`.
/// Polls every second up to `timeout_secs` (default 30).
pub async fn wait(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let id = p.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?
        .to_string();
    let since_count = p.get("since_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let timeout_secs = p.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30);

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
