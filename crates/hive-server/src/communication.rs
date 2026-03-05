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
    let rows = stmt.query_map(params![to_agent_id], |row| row_to_message(row))?;
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

pub fn get_agent(pool: &DbPool, id: &str) -> Result<Option<Agent>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, tags, connected_at, last_seen_at FROM agents WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_agent(row)?))
    } else {
        Ok(None)
    }
}

pub fn list_agents(pool: &DbPool) -> Result<Vec<Agent>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, tags, connected_at, last_seen_at FROM agents ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| row_to_agent(row))?;
    rows.map(|r| r.context("reading agent row"))
        .collect()
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
    let created_at_str: String = row.get(5)?;
    let delivered: i32 = row.get(4)?;
    Ok(PushMessage {
        id: row.get(0)?,
        from_agent_id: row.get(1)?,
        to_agent_id: row.get(2)?,
        content: row.get(3)?,
        delivered: delivered != 0,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    let tags_json: String = row.get(2)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let connected_at: Option<String> = row.get(3)?;
    let last_seen_at: Option<String> = row.get(4)?;
    Ok(Agent {
        id: row.get(0)?,
        name: row.get(1)?,
        tags,
        connected_at: connected_at.and_then(|s| s.parse().ok()),
        last_seen_at: last_seen_at.and_then(|s| s.parse().ok()),
    })
}
