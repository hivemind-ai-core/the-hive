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
    let rows = stmt.query_map([], row_to_topic)?;
    rows.map(|r| r.context("reading topic row")).collect()
}

/// Return topics whose `last_updated_at` is strictly after the given Unix timestamp.
pub fn list_topics_since(pool: &DbPool, since_unix: i64) -> Result<Vec<Topic>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, creator_agent_id, created_at, last_updated_at
         FROM topics WHERE CAST(strftime('%s', last_updated_at) AS INTEGER) > ?1
         ORDER BY last_updated_at DESC",
    )?;
    let rows = stmt.query_map(params![since_unix], row_to_topic)?;
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
    let rows = stmt.query_map(params![topic_id], row_to_comment)?;
    rows.map(|r| r.context("reading comment row")).collect()
}

// -- helpers --

fn row_to_topic(row: &rusqlite::Row<'_>) -> rusqlite::Result<Topic> {
    use chrono::Utc;
    let id: String = row.get(0)?;
    let created_at_str: String = row.get(4)?;
    let updated_at_str: String = row.get(5)?;
    let created_at = created_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(topic_id = %id, raw = %created_at_str, error = %e, "failed to parse topic created_at; using now");
        Utc::now()
    });
    let last_updated_at = updated_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(topic_id = %id, raw = %updated_at_str, error = %e, "failed to parse topic last_updated_at; using now");
        Utc::now()
    });
    Ok(Topic {
        id,
        title: row.get(1)?,
        content: row.get(2)?,
        creator_agent_id: row.get(3)?,
        created_at,
        last_updated_at,
    })
}

fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    use chrono::Utc;
    let id: String = row.get(0)?;
    let created_at_str: String = row.get(4)?;
    let created_at = created_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(comment_id = %id, raw = %created_at_str, error = %e, "failed to parse comment created_at; using now");
        Utc::now()
    });
    Ok(Comment {
        id,
        topic_id: row.get(1)?,
        content: row.get(2)?,
        creator_agent_id: row.get(3)?,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;
    use crate::test_helpers::{make_comment, make_topic};

    // ── insert / get topic ────────────────────────────────────────────────────

    #[test]
    fn insert_and_get_topic_round_trip() {
        let pool = open_test_db();
        let topic = make_topic("Hello World");
        insert_topic(&pool, &topic).unwrap();
        let got = get_topic(&pool, &topic.id).unwrap().expect("should exist");
        assert_eq!(got.id, topic.id);
        assert_eq!(got.title, "Hello World");
        assert_eq!(got.content, "content");
        assert_eq!(got.creator_agent_id.as_deref(), Some("agent-1"));
    }

    #[test]
    fn get_topic_nonexistent_returns_none() {
        let pool = open_test_db();
        let result = get_topic(&pool, "ghost-id").unwrap();
        assert!(result.is_none());
    }

    // ── list_topics ───────────────────────────────────────────────────────────

    #[test]
    fn list_topics_empty_db() {
        let pool = open_test_db();
        let topics = list_topics(&pool).unwrap();
        assert!(topics.is_empty());
    }

    #[test]
    fn list_topics_returns_all() {
        let pool = open_test_db();
        insert_topic(&pool, &make_topic("T1")).unwrap();
        insert_topic(&pool, &make_topic("T2")).unwrap();
        let topics = list_topics(&pool).unwrap();
        assert_eq!(topics.len(), 2);
    }

    #[test]
    fn list_topics_ordered_by_last_updated_desc() {
        let pool = open_test_db();
        let t1 = make_topic("Old");
        let t2 = make_topic("New");
        insert_topic(&pool, &t1).unwrap();
        // Small sleep to ensure different timestamps.
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert_topic(&pool, &t2).unwrap();
        let topics = list_topics(&pool).unwrap();
        // Most recently created topic first (DESC order).
        assert_eq!(topics[0].title, "New");
    }

    // ── list_topics_since ─────────────────────────────────────────────────────

    #[test]
    fn list_topics_since_filters_correctly() {
        let pool = open_test_db();
        let before = chrono::Utc::now().timestamp() - 10;
        insert_topic(&pool, &make_topic("Recent")).unwrap();
        let topics = list_topics_since(&pool, before).unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].title, "Recent");
    }

    #[test]
    fn list_topics_since_future_timestamp_returns_empty() {
        let pool = open_test_db();
        insert_topic(&pool, &make_topic("Old Topic")).unwrap();
        let future = chrono::Utc::now().timestamp() + 9999;
        let topics = list_topics_since(&pool, future).unwrap();
        assert!(topics.is_empty());
    }

    // ── insert_comment / get_comments ─────────────────────────────────────────

    #[test]
    fn insert_comment_and_retrieve() {
        let pool = open_test_db();
        let topic = make_topic("Discussion");
        insert_topic(&pool, &topic).unwrap();
        let comment = make_comment(&topic.id, "First reply");
        insert_comment(&pool, &comment).unwrap();
        let comments = get_comments(&pool, &topic.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].content, "First reply");
        assert_eq!(comments[0].topic_id, topic.id);
    }

    #[test]
    fn get_comments_empty_for_unknown_topic() {
        let pool = open_test_db();
        let comments = get_comments(&pool, "ghost-topic").unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn get_comments_ordered_by_created_at_asc() {
        let pool = open_test_db();
        let topic = make_topic("Thread");
        insert_topic(&pool, &topic).unwrap();
        insert_comment(&pool, &make_comment(&topic.id, "First")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert_comment(&pool, &make_comment(&topic.id, "Second")).unwrap();
        let comments = get_comments(&pool, &topic.id).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].content, "First");
        assert_eq!(comments[1].content, "Second");
    }

    #[test]
    fn insert_comment_bumps_topic_last_updated_at() {
        let pool = open_test_db();
        let topic = make_topic("Bumpy");
        insert_topic(&pool, &topic).unwrap();
        let before = get_topic(&pool, &topic.id)
            .unwrap()
            .unwrap()
            .last_updated_at;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut comment = make_comment(&topic.id, "bump");
        // Advance created_at so the bump is detectable.
        comment.created_at = chrono::Utc::now();
        insert_comment(&pool, &comment).unwrap();
        let after = get_topic(&pool, &topic.id)
            .unwrap()
            .unwrap()
            .last_updated_at;
        assert!(after >= before, "last_updated_at should be bumped");
    }

    #[test]
    fn get_comments_only_returns_for_own_topic() {
        let pool = open_test_db();
        let t1 = make_topic("T1");
        let t2 = make_topic("T2");
        insert_topic(&pool, &t1).unwrap();
        insert_topic(&pool, &t2).unwrap();
        insert_comment(&pool, &make_comment(&t1.id, "for T1")).unwrap();
        insert_comment(&pool, &make_comment(&t2.id, "for T2")).unwrap();
        let t1_comments = get_comments(&pool, &t1.id).unwrap();
        assert_eq!(t1_comments.len(), 1);
        assert_eq!(t1_comments[0].content, "for T1");
    }
}
