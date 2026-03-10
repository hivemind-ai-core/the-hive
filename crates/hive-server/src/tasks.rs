//! Database operations for tasks.

use anyhow::{Context, Result};
use chrono::Utc;
use hive_core::types::{Task, TaskStatus};
use rusqlite::params;

use crate::db::DbPool;

const STATUS_PENDING: &str = "pending";
const STATUS_IN_PROGRESS: &str = "in-progress";
const STATUS_DONE: &str = "done";
const STATUS_BLOCKED: &str = "blocked";
const STATUS_CANCELLED: &str = "cancelled";

pub fn insert_task(pool: &DbPool, task: &Task) -> Result<()> {
    let conn = pool.get()?;
    let tags = serde_json::to_string(&task.tags).context("serializing tags")?;
    let status = task.status.to_string();
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
    let status = task.status.to_string();
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

/// List pending tasks for dispatch with agent-tag semantics:
/// - No tag: return all pending tasks.
/// - Tag "X": return pending tasks tagged "X" OR tasks with no tags (untagged work).
///   Tasks with an explicit tag are only eligible for agents carrying that tag.
fn list_pending_for_dispatch(pool: &DbPool, tag: Option<&str>) -> Result<Vec<Task>> {
    let conn = pool.get()?;
    let sql = if tag.is_some() {
        "SELECT id, title, description, status, assigned_agent_id, tags, result, position, created_at, updated_at
         FROM tasks
         WHERE status = ?1
           AND (tags = '[]' OR tags IS NULL OR tags = ''
                OR EXISTS (SELECT 1 FROM json_each(tags) WHERE value = ?2))
         ORDER BY position ASC, created_at ASC"
    } else {
        "SELECT id, title, description, status, assigned_agent_id, tags, result, position, created_at, updated_at
         FROM tasks
         WHERE status = ?1
         ORDER BY position ASC, created_at ASC"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<Task> = if let Some(t) = tag {
        stmt.query_map(rusqlite::params![STATUS_PENDING, t], row_to_task)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(rusqlite::params![STATUS_PENDING], row_to_task)?
            .filter_map(|r| r.ok())
            .collect()
    };
    Ok(rows)
}

/// Find the next pending task for an agent, respecting dependencies and optional tag filter.
/// Assigns the task to `agent_id` and sets status to `in-progress`.
///
/// Dispatch tag semantics (different from task.list):
/// - If `tag` is None: claim any pending task regardless of tags.
/// - If `tag` is Some("rust"): claim tasks tagged "rust" OR tasks with NO tags.
///   Tasks without tags are untagged work, claimable by any agent.
///   Tasks with an explicit tag are ONLY claimable by an agent carrying that tag.
pub fn get_next(pool: &DbPool, agent_id: &str, tag: Option<&str>) -> Result<Option<Task>> {
    // Fetch pending tasks with dispatch-specific tag semantics.
    let pending = list_pending_for_dispatch(pool, tag)?;

    // For each candidate check that all its dependencies are `done`.
    let conn = pool.get()?;
    for mut task in pending {
        let unfinished: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks t ON t.id = td.depends_on_id
             WHERE td.task_id = ?1 AND t.status != ?2",
            rusqlite::params![task.id, STATUS_DONE],
            |row| row.get(0),
        )?;

        if unfinished == 0 {
            // Claim it.
            task.status = TaskStatus::InProgress;
            task.assigned_agent_id = Some(agent_id.to_string());
            conn.execute(
                "UPDATE tasks SET status=?3, assigned_agent_id=?2, updated_at=datetime('now')
                 WHERE id=?1",
                rusqlite::params![task.id, agent_id, STATUS_IN_PROGRESS],
            )?;
            return Ok(Some(task));
        }
    }
    Ok(None)
}

/// Reset all in-progress tasks assigned to `agent_id` back to pending.
/// Returns the number of tasks reset.
pub fn reset_in_progress_for_agent(pool: &DbPool, agent_id: &str) -> Result<usize> {
    let conn = pool.get()?;
    let count = conn
        .execute(
            "UPDATE tasks SET status=?2, assigned_agent_id=NULL, updated_at=datetime('now')
         WHERE status=?3 AND assigned_agent_id=?1",
            rusqlite::params![agent_id, STATUS_PENDING, STATUS_IN_PROGRESS],
        )
        .context("resetting orphaned tasks")?;
    Ok(count)
}

/// Mark a task done and optionally store a result string.
#[allow(clippy::needless_pass_by_value)] // result is consumed by rusqlite params
pub fn complete(pool: &DbPool, task_id: &str, result: Option<String>) -> Result<Task> {
    {
        let conn = pool.get()?;
        conn.execute(
            "UPDATE tasks SET status=?3, result=?2, updated_at=datetime('now') WHERE id=?1",
            rusqlite::params![task_id, result, STATUS_DONE],
        )
        .context("completing task")?;
    } // release lock before calling get_task

    get_task(pool, task_id)?.ok_or_else(|| anyhow::anyhow!("task not found after complete"))
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
        // Exact tag match: task must have the specific tag.
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

    let rows = stmt.query_map(refs.as_slice(), row_to_task)?;
    rows.map(|r| r.context("reading task row")).collect()
}

// -- helpers --

fn str_to_status(s: &str) -> TaskStatus {
    match s {
        STATUS_IN_PROGRESS => TaskStatus::InProgress,
        STATUS_DONE => TaskStatus::Done,
        STATUS_BLOCKED => TaskStatus::Blocked,
        STATUS_CANCELLED => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get(0)?;
    let tags_json: String = row.get(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_else(|e| {
        tracing::warn!(task_id = %id, raw = %tags_json, error = %e, "failed to parse task tags; using empty vec");
        vec![]
    });
    let status_str: String = row.get(3)?;
    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;
    let created_at = created_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(task_id = %id, raw = %created_at_str, error = %e, "failed to parse task created_at; using now");
        Utc::now()
    });
    let updated_at = updated_at_str.parse().unwrap_or_else(|e| {
        tracing::warn!(task_id = %id, raw = %updated_at_str, error = %e, "failed to parse task updated_at; using now");
        Utc::now()
    });
    Ok(Task {
        id,
        title: row.get(1)?,
        description: row.get(2)?,
        status: str_to_status(&status_str),
        assigned_agent_id: row.get(4)?,
        tags,
        result: row.get(6)?,
        position: row.get(7)?,
        created_at,
        updated_at,
    })
}

/// Split a task into ordered subtasks. The original is cancelled.
/// Returns the newly created subtasks in order.
pub fn split(pool: &DbPool, parent_id: &str, subtasks: Vec<Task>) -> Result<Vec<Task>> {
    get_task(pool, parent_id)?.ok_or_else(|| anyhow::anyhow!("task not found: {parent_id}"))?;

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
        "UPDATE tasks SET status=?2, updated_at=datetime('now') WHERE id=?1",
        rusqlite::params![parent_id, STATUS_CANCELLED],
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

    let active_sql =
        format!("SELECT id FROM tasks WHERE status NOT IN ('{STATUS_DONE}','{STATUS_CANCELLED}')");
    let mut stmt = conn.prepare(&active_sql)?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let mut in_degree: HashMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    {
        let deps_sql = format!(
            "SELECT task_id, depends_on_id FROM task_dependencies
             WHERE task_id IN (SELECT id FROM tasks WHERE status NOT IN ('{STATUS_DONE}','{STATUS_CANCELLED}'))"
        );
        let mut stmt = conn.prepare(&deps_sql)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;
    use crate::test_helpers::{make_tagged_task, make_task};

    // ── insert / get ─────────────────────────────────────────────────────────

    #[test]
    fn insert_and_get_round_trip() {
        let pool = open_test_db();
        let task = make_task("Round Trip");
        insert_task(&pool, &task).unwrap();
        let got = get_task(&pool, &task.id).unwrap().expect("should exist");
        assert_eq!(got.id, task.id);
        assert_eq!(got.title, "Round Trip");
        assert_eq!(got.status, TaskStatus::Pending);
        assert!(got.assigned_agent_id.is_none());
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let pool = open_test_db();
        let result = get_task(&pool, "nonexistent-id").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn insert_task_with_description_and_tags() {
        let pool = open_test_db();
        let mut task = make_task("Tagged");
        task.description = Some("desc".to_string());
        task.tags = vec!["rust".to_string(), "backend".to_string()];
        insert_task(&pool, &task).unwrap();
        let got = get_task(&pool, &task.id).unwrap().unwrap();
        assert_eq!(got.description.as_deref(), Some("desc"));
        assert!(got.tags.contains(&"rust".to_string()));
        assert!(got.tags.contains(&"backend".to_string()));
    }

    // ── update ────────────────────────────────────────────────────────────────

    #[test]
    fn update_persists_changes() {
        let pool = open_test_db();
        let mut task = make_task("Original");
        insert_task(&pool, &task).unwrap();
        task.title = "Updated".to_string();
        task.description = Some("new desc".to_string());
        update_task(&pool, &task).unwrap();
        let got = get_task(&pool, &task.id).unwrap().unwrap();
        assert_eq!(got.title, "Updated");
        assert_eq!(got.description.as_deref(), Some("new desc"));
    }

    // ── list_tasks ────────────────────────────────────────────────────────────

    #[test]
    fn list_all_tasks_no_filter() {
        let pool = open_test_db();
        insert_task(&pool, &make_task("T1")).unwrap();
        insert_task(&pool, &make_task("T2")).unwrap();
        let tasks = list_tasks(&pool, None, None, None).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_tasks_filter_by_status() {
        let pool = open_test_db();
        let task = make_task("Pending One");
        insert_task(&pool, &task).unwrap();
        // Claim it so it becomes in-progress.
        get_next(&pool, "agent-x", None).unwrap();
        let pending = list_tasks(&pool, Some("pending"), None, None).unwrap();
        assert!(pending.is_empty());
        let in_prog = list_tasks(&pool, Some("in-progress"), None, None).unwrap();
        assert_eq!(in_prog.len(), 1);
    }

    #[test]
    fn list_tasks_filter_by_tag() {
        let pool = open_test_db();
        insert_task(&pool, &make_tagged_task("Rust Task", &["rust"])).unwrap();
        insert_task(&pool, &make_tagged_task("Python Task", &["python"])).unwrap();
        insert_task(&pool, &make_task("Untagged")).unwrap();
        let rust_tasks = list_tasks(&pool, None, Some("rust"), None).unwrap();
        assert_eq!(rust_tasks.len(), 1);
        assert_eq!(rust_tasks[0].title, "Rust Task");
    }

    #[test]
    fn list_tasks_filter_by_assigned_agent() {
        let pool = open_test_db();
        insert_task(&pool, &make_task("A")).unwrap();
        insert_task(&pool, &make_task("B")).unwrap();
        get_next(&pool, "agent-1", None).unwrap();
        let assigned = list_tasks(&pool, None, None, Some("agent-1")).unwrap();
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].assigned_agent_id.as_deref(), Some("agent-1"));
    }

    // ── get_next ──────────────────────────────────────────────────────────────

    #[test]
    fn get_next_returns_none_when_no_tasks() {
        let pool = open_test_db();
        let result = get_next(&pool, "agent-1", None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_next_claims_pending_task() {
        let pool = open_test_db();
        let task = make_task("Work Item");
        insert_task(&pool, &task).unwrap();
        let claimed = get_next(&pool, "agent-1", None)
            .unwrap()
            .expect("should get a task");
        assert_eq!(claimed.id, task.id);
        assert_eq!(claimed.status, TaskStatus::InProgress);
        assert_eq!(claimed.assigned_agent_id.as_deref(), Some("agent-1"));
    }

    #[test]
    fn get_next_does_not_return_task_blocked_by_dep() {
        let pool = open_test_db();
        let dep = make_task("Dependency");
        let blocked = make_task("Blocked Task");
        insert_task(&pool, &dep).unwrap();
        insert_task(&pool, &blocked).unwrap();
        set_dependency(&pool, &blocked.id, &dep.id).unwrap();
        // Only the dependency is claimable.
        let first = get_next(&pool, "agent-1", None)
            .unwrap()
            .expect("should get dep");
        assert_eq!(first.id, dep.id);
        // Dependency is in-progress, not done → blocked task still unavailable.
        let second = get_next(&pool, "agent-2", None).unwrap();
        assert!(
            second.is_none(),
            "blocked task should not be claimable while dep is in-progress"
        );
    }

    #[test]
    fn get_next_unblocks_after_dependency_done() {
        let pool = open_test_db();
        let dep = make_task("Dep");
        let work = make_task("Blocked");
        insert_task(&pool, &dep).unwrap();
        insert_task(&pool, &work).unwrap();
        set_dependency(&pool, &work.id, &dep.id).unwrap();
        get_next(&pool, "agent-1", None).unwrap(); // claims dep
        complete(&pool, &dep.id, None).unwrap();
        let next = get_next(&pool, "agent-2", None)
            .unwrap()
            .expect("should be unblocked");
        assert_eq!(next.id, work.id);
    }

    #[test]
    fn get_next_respects_tag_filter() {
        let pool = open_test_db();
        insert_task(&pool, &make_tagged_task("Rust Task", &["rust"])).unwrap();
        insert_task(&pool, &make_tagged_task("Python Task", &["python"])).unwrap();
        let claimed = get_next(&pool, "agent-rust", Some("rust"))
            .unwrap()
            .expect("should get task");
        assert_eq!(claimed.title, "Rust Task");
        // Python task stays pending.
        let remaining = list_tasks(&pool, Some("pending"), None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "Python Task");
    }

    #[test]
    fn get_next_tagged_agent_can_claim_untagged_task() {
        let pool = open_test_db();
        insert_task(&pool, &make_task("Untagged Task")).unwrap();
        // A tagged agent can claim untagged tasks.
        let claimed = get_next(&pool, "agent-rust", Some("rust")).unwrap();
        assert!(claimed.is_some(), "tagged agent should claim untagged task");
    }

    // ── complete ──────────────────────────────────────────────────────────────

    #[test]
    fn complete_marks_task_done_with_result() {
        let pool = open_test_db();
        let task = make_task("Finish Me");
        insert_task(&pool, &task).unwrap();
        let done = complete(&pool, &task.id, Some("done result".to_string())).unwrap();
        assert_eq!(done.status, TaskStatus::Done);
        assert_eq!(done.result.as_deref(), Some("done result"));
    }

    #[test]
    fn complete_without_result() {
        let pool = open_test_db();
        let task = make_task("No Result");
        insert_task(&pool, &task).unwrap();
        let done = complete(&pool, &task.id, None).unwrap();
        assert_eq!(done.status, TaskStatus::Done);
        assert!(done.result.is_none());
    }

    #[test]
    fn complete_nonexistent_task_returns_error() {
        let pool = open_test_db();
        let result = complete(&pool, "no-such-id", None);
        assert!(result.is_err());
    }

    // ── reset_in_progress_for_agent ───────────────────────────────────────────

    #[test]
    fn reset_in_progress_resets_and_returns_count() {
        let pool = open_test_db();
        insert_task(&pool, &make_task("T1")).unwrap();
        insert_task(&pool, &make_task("T2")).unwrap();
        get_next(&pool, "agent-drop", None).unwrap();
        get_next(&pool, "agent-drop", None).unwrap();
        let count = reset_in_progress_for_agent(&pool, "agent-drop").unwrap();
        assert_eq!(count, 2);
        let pending = list_tasks(&pool, Some("pending"), None, None).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|t| t.assigned_agent_id.is_none()));
    }

    #[test]
    fn reset_in_progress_only_affects_target_agent() {
        let pool = open_test_db();
        insert_task(&pool, &make_task("A")).unwrap();
        insert_task(&pool, &make_task("B")).unwrap();
        get_next(&pool, "agent-1", None).unwrap();
        get_next(&pool, "agent-2", None).unwrap();
        reset_in_progress_for_agent(&pool, "agent-1").unwrap();
        let in_prog = list_tasks(&pool, Some("in-progress"), None, None).unwrap();
        assert_eq!(in_prog.len(), 1);
        assert_eq!(in_prog[0].assigned_agent_id.as_deref(), Some("agent-2"));
    }

    #[test]
    fn reset_returns_zero_when_no_tasks_match() {
        let pool = open_test_db();
        let count = reset_in_progress_for_agent(&pool, "ghost-agent").unwrap();
        assert_eq!(count, 0);
    }

    // ── split ─────────────────────────────────────────────────────────────────

    #[test]
    fn split_cancels_parent_creates_subtasks() {
        let pool = open_test_db();
        let parent = make_task("Parent");
        insert_task(&pool, &parent).unwrap();
        let created = split(
            &pool,
            &parent.id,
            vec![make_task("S1"), make_task("S2"), make_task("S3")],
        )
        .unwrap();
        assert_eq!(created.len(), 3);
        let p = get_task(&pool, &parent.id).unwrap().unwrap();
        assert_eq!(p.status, TaskStatus::Cancelled);
    }

    #[test]
    fn split_creates_sequential_chain_deps() {
        let pool = open_test_db();
        let parent = make_task("Parent");
        insert_task(&pool, &parent).unwrap();
        let s1 = make_task("S1");
        let s2 = make_task("S2");
        let s1_id = s1.id.clone();
        let s2_id = s2.id.clone();
        split(&pool, &parent.id, vec![s1, s2]).unwrap();
        // S1 has no deps → claimable first.
        let first = get_next(&pool, "agent", None)
            .unwrap()
            .expect("should get S1");
        assert_eq!(first.id, s1_id);
        // S2 depends on S1 which is in-progress → not claimable.
        let second = get_next(&pool, "agent2", None).unwrap();
        assert!(
            second.as_ref().map(|t| t.id != s2_id).unwrap_or(true),
            "S2 should be blocked until S1 is done"
        );
    }

    #[test]
    fn split_nonexistent_parent_errors() {
        let pool = open_test_db();
        let result = split(&pool, "ghost-id", vec![make_task("Sub")]);
        assert!(result.is_err());
    }

    // ── set_dependency ────────────────────────────────────────────────────────

    #[test]
    fn set_dependency_self_reference_errors() {
        let pool = open_test_db();
        let task = make_task("Self");
        insert_task(&pool, &task).unwrap();
        let result = set_dependency(&pool, &task.id, &task.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("itself"));
    }

    #[test]
    fn set_dependency_cycle_errors() {
        let pool = open_test_db();
        let a = make_task("A");
        let b = make_task("B");
        insert_task(&pool, &a).unwrap();
        insert_task(&pool, &b).unwrap();
        set_dependency(&pool, &a.id, &b.id).unwrap(); // A → B
        let result = set_dependency(&pool, &b.id, &a.id); // B → A (cycle)
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("circular"));
    }

    #[test]
    fn cycle_detection_does_not_persist_bad_edge() {
        let pool = open_test_db();
        let a = make_task("A");
        let b = make_task("B");
        insert_task(&pool, &a).unwrap();
        insert_task(&pool, &b).unwrap();
        set_dependency(&pool, &a.id, &b.id).unwrap();
        let _ = set_dependency(&pool, &b.id, &a.id); // cycle, should fail
                                                     // A should still have exactly one dependency (b), and b should have none.
        let conn = pool.get().unwrap();
        let a_dep_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_dependencies WHERE task_id = ?1",
                rusqlite::params![a.id],
                |r| r.get(0),
            )
            .unwrap();
        let b_dep_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_dependencies WHERE task_id = ?1",
                rusqlite::params![b.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(a_dep_count, 1);
        assert_eq!(b_dep_count, 0, "cycle edge should be rolled back");
    }

    #[test]
    fn set_dependency_reorders_positions() {
        let pool = open_test_db();
        let a = make_task("A");
        let b = make_task("B");
        insert_task(&pool, &a).unwrap();
        insert_task(&pool, &b).unwrap();
        set_dependency(&pool, &b.id, &a.id).unwrap();
        let all = list_tasks(&pool, None, None, None).unwrap();
        let pos_a = all.iter().find(|t| t.id == a.id).unwrap().position;
        let pos_b = all.iter().find(|t| t.id == b.id).unwrap().position;
        assert!(pos_a < pos_b, "A must be positioned before B");
    }
}
