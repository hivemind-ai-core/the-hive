//! Database operations for the message board (topics + comments).

use anyhow::{Context, Result};
use hive_core::types::{Comment, Topic};
use rusqlite::params;

use crate::db::DbPool;

pub fn insert_topic(pool: &DbPool, topic: &Topic) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO topics (id, title, content, creator_agent_id, created_at, last_updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            topic.id,
            topic.title,
            topic.content,
            topic.creator_agent_id,
            topic.created_at.to_rfc3339(),
            topic.last_updated_at.to_rfc3339(),
        ],
    )
    .context("inserting topic")?;
    Ok(())
}

pub fn get_topic(pool: &DbPool, id: &str) -> Result<Option<Topic>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, creator_agent_id, created_at, last_updated_at
         FROM topics WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_topic(row)?))
    } else {
        Ok(None)
    }
}

pub fn list_topics(pool: &DbPool) -> Result<Vec<Topic>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, creator_agent_id, created_at, last_updated_at
         FROM topics ORDER BY last_updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| row_to_topic(row))?;
    rows.map(|r| r.context("reading topic row"))
        .collect()
}

/// Return topics whose `last_updated_at` is strictly after the given Unix timestamp.
pub fn list_topics_since(pool: &DbPool, since_unix: i64) -> Result<Vec<Topic>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, creator_agent_id, created_at, last_updated_at
         FROM topics WHERE last_updated_at > datetime(?1, 'unixepoch')
         ORDER BY last_updated_at DESC",
    )?;
    let rows = stmt.query_map(params![since_unix], |row| row_to_topic(row))?;
    rows.map(|r| r.context("reading topic row")).collect()
}

pub fn insert_comment(pool: &DbPool, comment: &Comment) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO comments (id, topic_id, content, creator_agent_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            comment.id,
            comment.topic_id,
            comment.content,
            comment.creator_agent_id,
            comment.created_at.to_rfc3339(),
        ],
    )
    .context("inserting comment")?;
    // Bump topic's last_updated_at
    conn.execute(
        "UPDATE topics SET last_updated_at = ?2 WHERE id = ?1",
        params![comment.topic_id, comment.created_at.to_rfc3339()],
    )
    .context("updating topic last_updated_at")?;
    Ok(())
}

pub fn get_comments(pool: &DbPool, topic_id: &str) -> Result<Vec<Comment>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, topic_id, content, creator_agent_id, created_at
         FROM comments WHERE topic_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![topic_id], |row| row_to_comment(row))?;
    rows.map(|r| r.context("reading comment row"))
        .collect()
}

// -- helpers --

fn row_to_topic(row: &rusqlite::Row<'_>) -> rusqlite::Result<Topic> {
    use chrono::Utc;
    let created_at_str: String = row.get(4)?;
    let updated_at_str: String = row.get(5)?;
    Ok(Topic {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        creator_agent_id: row.get(3)?,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
        last_updated_at: updated_at_str.parse().unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    use chrono::Utc;
    let created_at_str: String = row.get(4)?;
    Ok(Comment {
        id: row.get(0)?,
        topic_id: row.get(1)?,
        content: row.get(2)?,
        creator_agent_id: row.get(3)?,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
    })
}
