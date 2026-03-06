//! Database operations for tasks.

use anyhow::{Context, Result};
use chrono::Utc;
use hive_core::types::{Task, TaskStatus};
use rusqlite::params;

use crate::db::DbPool;

pub fn insert_task(pool: &DbPool, task: &Task) -> Result<()> {
    let conn = pool.get()?;
    let tags = serde_json::to_string(&task.tags).context("serializing tags")?;
    let status = status_to_str(task.status);
    conn.execute(
        "INSERT INTO tasks (id, title, description, status, assigned_agent_id, tags, result, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            task.id,
            task.title,
            task.description,
            status,
            task.assigned_agent_id,
            tags,
            task.result,
            task.position,
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
        ],
    )
    .context("inserting task")?;
    Ok(())
}

pub fn get_task(pool: &DbPool, id: &str) -> Result<Option<Task>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, description, status, assigned_agent_id, tags, result, position, created_at, updated_at
         FROM tasks WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_task(row)?))
    } else {
        Ok(None)
    }
}

pub fn update_task(pool: &DbPool, task: &Task) -> Result<()> {
    let conn = pool.get()?;
    let tags = serde_json::to_string(&task.tags).context("serializing tags")?;
    let status = status_to_str(task.status);
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET title=?2, description=?3, status=?4, assigned_agent_id=?5,
         tags=?6, result=?7, position=?8, updated_at=?9 WHERE id=?1",
        params![
            task.id,
            task.title,
            task.description,
            status,
            task.assigned_agent_id,
            tags,
            task.result,
            task.position,
            now,
        ],
    )
    .context("updating task")?;
    Ok(())
}

/// Find the next pending task for an agent, respecting dependencies and optional tag filter.
/// Assigns the task to `agent_id` and sets status to `in-progress`.
pub fn get_next(pool: &DbPool, agent_id: &str, tag: Option<&str>) -> Result<Option<Task>> {
    // Fetch all pending tasks, ordered by position then created_at.
    let pending = list_tasks(pool, Some("pending"), tag, None)?;

    // For each candidate check that all its dependencies are `done`.
    let conn = pool.get()?;
    for mut task in pending {
        let unfinished: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks t ON t.id = td.depends_on_id
             WHERE td.task_id = ?1 AND t.status != 'done'",
            rusqlite::params![task.id],
            |row| row.get(0),
        )?;

        if unfinished == 0 {
            // Claim it.
            task.status = TaskStatus::InProgress;
            task.assigned_agent_id = Some(agent_id.to_string());
            conn.execute(
                "UPDATE tasks SET status='in-progress', assigned_agent_id=?2, updated_at=datetime('now')
                 WHERE id=?1",
                rusqlite::params![task.id, agent_id],
            )?;
            return Ok(Some(task));
        }
    }
    Ok(None)
}

/// Mark a task done and optionally store a result string.
pub fn complete(pool: &DbPool, task_id: &str, result: Option<String>) -> Result<Task> {
    {
        let conn = pool.get()?;
        conn.execute(
            "UPDATE tasks SET status='done', result=?2, updated_at=datetime('now') WHERE id=?1",
            rusqlite::params![task_id, result],
        )
        .context("completing task")?;
    } // release lock before calling get_task

    get_task(pool, task_id)?
        .ok_or_else(|| anyhow::anyhow!("task not found after complete"))
}

pub fn insert_dependency(pool: &DbPool, task_id: &str, depends_on_id: &str) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
        params![task_id, depends_on_id],
    )
    .context("inserting task dependency")?;
    Ok(())
}

/// List tasks with optional filters. Any `None` filter is ignored.
pub fn list_tasks(
    pool: &DbPool,
    status: Option<&str>,
    tag: Option<&str>,
    assigned_agent_id: Option<&str>,
) -> Result<Vec<Task>> {
    let conn = pool.get()?;

    let mut conditions: Vec<&str> = Vec::new();
    let mut bound_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if status.is_some() {
        conditions.push("status = ?");
    }
    if assigned_agent_id.is_some() {
        conditions.push("assigned_agent_id = ?");
    }
    if tag.is_some() {
        conditions.push("EXISTS (SELECT 1 FROM json_each(tags) WHERE value = ?)");
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, title, description, status, assigned_agent_id, tags, result, position, created_at, updated_at
         FROM tasks{where_clause} ORDER BY position ASC, created_at ASC"
    );

    if let Some(s) = status {
        bound_params.push(Box::new(s.to_string()));
    }
    if let Some(a) = assigned_agent_id {
        bound_params.push(Box::new(a.to_string()));
    }
    if let Some(t) = tag {
        bound_params.push(Box::new(t.to_string()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = bound_params.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(refs.as_slice(), |row| row_to_task(row))?;
    rows.map(|r| r.context("reading task row")).collect()
}


// -- helpers --

fn status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Done => "done",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn str_to_status(s: &str) -> TaskStatus {
    match s {
        "in-progress" => TaskStatus::InProgress,
        "done" => TaskStatus::Done,
        "blocked" => TaskStatus::Blocked,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let tags_json: String = row.get(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let status_str: String = row.get(3)?;
    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        status: str_to_status(&status_str),
        assigned_agent_id: row.get(4)?,
        tags,
        result: row.get(6)?,
        position: row.get(7)?,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
        updated_at: updated_at_str.parse().unwrap_or_else(|_| Utc::now()),
    })
}

/// Split a task into ordered subtasks. The original is cancelled.
/// Returns the newly created subtasks in order.
pub fn split(pool: &DbPool, parent_id: &str, subtasks: Vec<Task>) -> Result<Vec<Task>> {
    get_task(pool, parent_id)?
        .ok_or_else(|| anyhow::anyhow!("task not found: {parent_id}"))?;

    let mut created: Vec<Task> = Vec::with_capacity(subtasks.len());
    let mut prev_id: Option<String> = None;

    for task in subtasks {
        insert_task(pool, &task)?;
        if let Some(dep_id) = &prev_id {
            insert_dependency(pool, &task.id, dep_id)?;
        }
        prev_id = Some(task.id.clone());
        created.push(task);
    }

    // Cancel the original.
    let conn = pool.get()?;
    conn.execute(
        "UPDATE tasks SET status='cancelled', updated_at=datetime('now') WHERE id=?1",
        rusqlite::params![parent_id],
    )?;

    Ok(created)
}

/// Add a dependency edge then reorder positions via topological sort.
/// Returns an error (and does not persist the edge) if a cycle would be created.
pub fn set_dependency(pool: &DbPool, task_id: &str, depends_on_id: &str) -> Result<()> {
    if task_id == depends_on_id {
        anyhow::bail!("a task cannot depend on itself");
    }
    insert_dependency(pool, task_id, depends_on_id)?;
    if let Err(e) = topological_reorder(pool) {
        // Cycle detected — remove the edge we just inserted.
        let conn = pool.get()?;
        let _ = conn.execute(
            "DELETE FROM task_dependencies WHERE task_id=?1 AND depends_on_id=?2",
            rusqlite::params![task_id, depends_on_id],
        );
        return Err(e);
    }
    Ok(())
}

fn topological_reorder(pool: &DbPool) -> Result<()> {
    use std::collections::{HashMap, VecDeque};

    let conn = pool.get()?;

    let mut stmt = conn.prepare(
        "SELECT id FROM tasks WHERE status NOT IN ('done','cancelled')",
    )?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let mut in_degree: HashMap<String, usize> =
        ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    {
        let mut stmt = conn.prepare(
            "SELECT task_id, depends_on_id FROM task_dependencies
             WHERE task_id IN (SELECT id FROM tasks WHERE status NOT IN ('done','cancelled'))",
        )?;
        let edges: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;

        for (task_id, dep_id) in edges {
            *in_degree.entry(task_id.clone()).or_insert(0) += 1;
            dependents.entry(dep_id).or_default().push(task_id);
        }
    }

    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let mut order: Vec<String> = Vec::with_capacity(ids.len());
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        for dep in dependents.get(&id).cloned().unwrap_or_default() {
            let d = in_degree.entry(dep.clone()).or_insert(0);
            *d -= 1;
            if *d == 0 {
                queue.push_back(dep);
            }
        }
    }

    // Cycle detection: if not all nodes were processed, some form a cycle.
    if order.len() != ids.len() {
        use std::collections::HashSet;
        let processed: HashSet<&String> = order.iter().collect();
        let cycle: Vec<&String> = ids.iter().filter(|id| !processed.contains(id)).collect();
        anyhow::bail!("circular dependency detected among tasks: {:?}", cycle);
    }

    for (pos, id) in order.iter().enumerate() {
        conn.execute(
            "UPDATE tasks SET position=?2 WHERE id=?1",
            rusqlite::params![id, pos as i32],
        )?;
    }

    Ok(())
}
