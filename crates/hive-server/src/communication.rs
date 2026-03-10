//! Database operations for push messages and agent registry.

use anyhow::{Context, Result};
use chrono::Utc;
use hive_core::types::{Agent, PushMessage};
use rusqlite::params;

use crate::db::DbPool;

// -- Push messages --

pub fn insert_message(pool: &DbPool, msg: &PushMessage) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO push_messages (id, from_agent_id, to_agent_id, content, delivered, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            msg.id,
            msg.from_agent_id,
            msg.to_agent_id,
            msg.content,
            msg.delivered as i32,
            msg.created_at.to_rfc3339(),
        ],
    )
    .context("inserting push message")?;
    Ok(())
}

/// Fetch undelivered messages for an agent, ordered oldest first.
pub fn pending_messages(pool: &DbPool, to_agent_id: &str) -> Result<Vec<PushMessage>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, from_agent_id, to_agent_id, content, delivered, created_at
         FROM push_messages WHERE to_agent_id = ?1 AND delivered = 0
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![to_agent_id], row_to_message)?;
    rows.map(|r| r.context("reading push message row"))
        .collect()
}

pub fn mark_delivered(pool: &DbPool, message_id: &str) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE push_messages SET delivered = 1 WHERE id = ?1",
        params![message_id],
    )
    .context("marking message delivered")?;
    Ok(())
}

// -- Agents --

pub fn upsert_agent(pool: &DbPool, agent: &Agent) -> Result<()> {
    let conn = pool.get()?;
    let tags = serde_json::to_string(&agent.tags).context("serializing agent tags")?;
    conn.execute(
        "INSERT INTO agents (id, name, tags, connected_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             tags = excluded.tags,
             connected_at = excluded.connected_at,
             last_seen_at = excluded.last_seen_at",
        params![
            agent.id,
            agent.name,
            tags,
            agent.connected_at.map(|t| t.to_rfc3339()),
            agent.last_seen_at.map(|t| t.to_rfc3339()),
        ],
    )
    .context("upserting agent")?;
    Ok(())
}

pub fn list_agents(pool: &DbPool) -> Result<Vec<Agent>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, tags, connected_at, last_seen_at FROM agents ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], row_to_agent)?;
    rows.map(|r| r.context("reading agent row")).collect()
}

pub fn touch_agent(pool: &DbPool, id: &str) -> Result<()> {
    let conn = pool.get()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET last_seen_at = ?2 WHERE id = ?1",
        params![id, now],
    )
    .context("touching agent last_seen_at")?;
    Ok(())
}

// -- helpers --

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<PushMessage> {
    let id: String = row.get(0)?;
    let created_at_str: String = row.get(5)?;
    let delivered: i32 = row.get(4)?;
    let created_at = created_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(message_id = %id, raw = %created_at_str, error = %e, "failed to parse message created_at; using now");
        Utc::now()
    });
    Ok(PushMessage {
        id,
        from_agent_id: row.get(1)?,
        to_agent_id: row.get(2)?,
        content: row.get(3)?,
        delivered: delivered != 0,
        created_at,
    })
}

fn row_to_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    let agent_id: String = row.get(0)?;
    let tags_json: String = row.get(2)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_else(|e| {
        tracing::warn!(agent_id = %agent_id, raw = %tags_json, error = %e, "failed to parse agent tags; using empty vec");
        vec![]
    });
    let connected_at: Option<String> = row.get(3)?;
    let last_seen_at: Option<String> = row.get(4)?;
    Ok(Agent {
        id: agent_id,
        name: row.get(1)?,
        tags,
        connected_at: connected_at.and_then(|s| s.parse().ok()),
        last_seen_at: last_seen_at.and_then(|s| s.parse().ok()),
        capacity_max: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;
    use crate::test_helpers::{make_agent, make_msg};

    // ── push messages ─────────────────────────────────────────────────────────

    #[test]
    fn insert_and_pending_messages() {
        let pool = open_test_db();
        let msg = make_msg("agent-b", "agent-a", "hello");
        insert_message(&pool, &msg).unwrap();
        let pending = pending_messages(&pool, "agent-b").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content, "hello");
        assert_eq!(pending[0].from_agent_id.as_deref(), Some("agent-a"));
        assert!(!pending[0].delivered);
    }

    #[test]
    fn pending_messages_empty_when_none() {
        let pool = open_test_db();
        let msgs = pending_messages(&pool, "no-such-agent").unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn pending_messages_only_returns_undelivered() {
        let pool = open_test_db();
        let m1 = make_msg("agent-b", "agent-a", "first");
        let m2 = make_msg("agent-b", "agent-a", "second");
        insert_message(&pool, &m1).unwrap();
        insert_message(&pool, &m2).unwrap();
        mark_delivered(&pool, &m1.id).unwrap();
        let pending = pending_messages(&pool, "agent-b").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content, "second");
    }

    #[test]
    fn mark_delivered_updates_flag() {
        let pool = open_test_db();
        let msg = make_msg("agent-b", "agent-a", "ping");
        insert_message(&pool, &msg).unwrap();
        mark_delivered(&pool, &msg.id).unwrap();
        // No more pending messages.
        let pending = pending_messages(&pool, "agent-b").unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_messages_only_for_recipient() {
        let pool = open_test_db();
        insert_message(&pool, &make_msg("agent-b", "agent-a", "for B")).unwrap();
        insert_message(&pool, &make_msg("agent-c", "agent-a", "for C")).unwrap();
        let b_msgs = pending_messages(&pool, "agent-b").unwrap();
        assert_eq!(b_msgs.len(), 1);
        assert_eq!(b_msgs[0].content, "for B");
    }

    #[test]
    fn pending_messages_ordered_oldest_first() {
        let pool = open_test_db();
        let m1 = make_msg("agent-b", "agent-a", "first");
        let m2 = make_msg("agent-b", "agent-a", "second");
        insert_message(&pool, &m1).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert_message(&pool, &m2).unwrap();
        let pending = pending_messages(&pool, "agent-b").unwrap();
        assert_eq!(pending[0].content, "first");
        assert_eq!(pending[1].content, "second");
    }

    // ── agents ────────────────────────────────────────────────────────────────

    #[test]
    fn upsert_and_list_agents() {
        let pool = open_test_db();
        upsert_agent(&pool, &make_agent("alpha")).unwrap();
        upsert_agent(&pool, &make_agent("beta")).unwrap();
        let agents = list_agents(&pool).unwrap();
        assert_eq!(agents.len(), 2);
        // Ordered by name ASC.
        assert_eq!(agents[0].id, "alpha");
        assert_eq!(agents[1].id, "beta");
    }

    #[test]
    fn upsert_agent_updates_existing() {
        let pool = open_test_db();
        let mut agent = make_agent("x");
        upsert_agent(&pool, &agent).unwrap();
        agent.name = "Updated Name".to_string();
        agent.tags = vec!["rust".to_string()];
        upsert_agent(&pool, &agent).unwrap();
        let agents = list_agents(&pool).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "Updated Name");
        assert!(agents[0].tags.contains(&"rust".to_string()));
    }

    #[test]
    fn list_agents_empty_db() {
        let pool = open_test_db();
        let agents = list_agents(&pool).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn touch_agent_updates_last_seen_at() {
        let pool = open_test_db();
        let mut agent = make_agent("touchable");
        // Set last_seen_at to a known past time.
        agent.last_seen_at = Some(chrono::DateTime::from_timestamp(0, 0).unwrap());
        upsert_agent(&pool, &agent).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        touch_agent(&pool, "touchable").unwrap();
        let agents = list_agents(&pool).unwrap();
        let updated = agents.iter().find(|a| a.id == "touchable").unwrap();
        // last_seen_at should now be recent (after epoch 0).
        if let Some(lsa) = updated.last_seen_at {
            assert!(
                lsa.timestamp() > 1_000_000_000,
                "last_seen_at should be updated to now"
            );
        }
    }

    #[test]
    fn upsert_agent_with_tags() {
        let pool = open_test_db();
        let mut agent = make_agent("tagged-agent");
        agent.tags = vec!["rust".to_string(), "backend".to_string()];
        upsert_agent(&pool, &agent).unwrap();
        let agents = list_agents(&pool).unwrap();
        assert!(agents[0].tags.contains(&"rust".to_string()));
        assert!(agents[0].tags.contains(&"backend".to_string()));
    }
}
