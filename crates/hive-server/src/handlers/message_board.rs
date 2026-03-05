//! Handlers for topic.* WS methods.

use anyhow::Result;
use hive_core::types::Topic;
use serde::Deserialize;
use serde_json::Value;

use crate::{db::DbPool, message_board as db_mb};

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

pub fn comment(pool: &DbPool, params: Option<Value>) -> Result<Value> {
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
    Ok(serde_json::to_value(&comment)?)
}

/// Wait until the comment count for a topic exceeds `since_count`.
/// Polls every second up to `timeout_secs` (default 30).
pub fn wait(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let id = p.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;
    let since_count = p.get("since_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let timeout_secs = p.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let comments = db_mb::get_comments(pool, id)?;
        if comments.len() > since_count {
            let topic = db_mb::get_topic(pool, id)?
                .ok_or_else(|| anyhow::anyhow!("topic not found"))?;
            return Ok(serde_json::json!({ "topic": topic, "comments": comments }));
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for new comments");
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
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
