//! Handlers for task.* WS methods.

use anyhow::Result;
use hive_core::types::Task;
use serde::Deserialize;
use serde_json::Value;

use crate::{db::DbPool, tasks as db_tasks};

#[derive(Deserialize)]
struct CreateParams {
    title: String,
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

pub fn create(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p: CreateParams = serde_json::from_value(params.unwrap_or(Value::Null))?;
    if p.title.trim().is_empty() {
        anyhow::bail!("title must not be empty");
    }
    let task = Task::new(p.title, p.description, p.tags);
    db_tasks::insert_task(pool, &task)?;
    Ok(serde_json::to_value(&task)?)
}

#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
pub fn list(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let status = params.as_ref().and_then(|v| v.get("status")).and_then(|v| v.as_str()).map(str::to_owned);
    let tag = params.as_ref().and_then(|v| v.get("tag")).and_then(|v| v.as_str()).map(str::to_owned);
    let agent = params.as_ref().and_then(|v| v.get("assigned_agent_id")).and_then(|v| v.as_str()).map(str::to_owned);

    let tasks = db_tasks::list_tasks(
        pool,
        status.as_deref(),
        tag.as_deref(),
        agent.as_deref(),
    )?;
    Ok(serde_json::to_value(&tasks)?)
}

#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
pub fn get(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let id = params
        .as_ref()
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;

    match db_tasks::get_task(pool, id)? {
        Some(task) => Ok(serde_json::to_value(&task)?),
        None => anyhow::bail!("task not found"),
    }
}

pub fn update(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let id = p.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;

    let mut task = db_tasks::get_task(pool, id)?
        .ok_or_else(|| anyhow::anyhow!("task not found"))?;

    if let Some(desc) = p.get("description").and_then(|v| v.as_str()) {
        task.description = Some(desc.to_string());
    }
    if let Some(tags) = p.get("tags") {
        task.tags = serde_json::from_value(tags.clone())?;
    }
    if let Some(status_str) = p.get("status").and_then(|v| v.as_str()) {
        let new_status: hive_core::types::TaskStatus =
            serde_json::from_value(serde_json::json!(status_str))
                .map_err(|_| anyhow::anyhow!("unknown status: {status_str}"))?;
        validate_transition(task.status, new_status)?;
        task.status = new_status;
        // Reset to pending also unassigns the task.
        if new_status == hive_core::types::TaskStatus::Pending {
            task.assigned_agent_id = None;
        }
    }

    db_tasks::update_task(pool, &task)?;
    Ok(serde_json::to_value(&task)?)
}

pub fn split(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    use hive_core::types::Task as HiveTask;

    let p = params.unwrap_or(Value::Null);
    let id = p.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;
    let raw_subtasks = p.get("subtasks").and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("params.subtasks (array) is required"))?;

    let subtasks: Vec<HiveTask> = raw_subtasks.iter().map(|v| {
        if let Some(title) = v.as_str() {
            // Plain string — treat as title only.
            HiveTask::new(title.to_string(), None, vec![])
        } else {
            // Object with title, optional description and tags.
            let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let description = v.get("description").and_then(|d| d.as_str()).map(str::to_string);
            let tags: Vec<String> = v.get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            HiveTask::new(title, description, tags)
        }
    }).collect();

    let created = db_tasks::split(pool, id, subtasks)?;
    Ok(serde_json::to_value(&created)?)
}

pub fn set_dependency(pool: &DbPool, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let task_id = p.get("task_id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.task_id is required"))?;
    let depends_on_id = p.get("depends_on_id").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.depends_on_id is required"))?;
    db_tasks::set_dependency(pool, task_id, depends_on_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

#[allow(clippy::needless_pass_by_value)] // Params are passed owned from the WS dispatcher
pub fn get_next(pool: &DbPool, agent_id: &str, params: Option<Value>) -> Result<Value> {
    let tag = params.as_ref().and_then(|v| v.get("tag")).and_then(|v| v.as_str()).map(str::to_owned);
    match db_tasks::get_next(pool, agent_id, tag.as_deref())? {
        Some(task) => Ok(serde_json::to_value(&task)?),
        None => Ok(serde_json::json!(null)),
    }
}

pub fn complete(pool: &DbPool, agent_id: &str, params: Option<Value>) -> Result<Value> {
    let p = params.unwrap_or(Value::Null);
    let id = p.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("params.id is required"))?;
    let result = p.get("result").and_then(|v| v.as_str()).map(str::to_string);

    db_tasks::complete(pool, id, result)?;
    let next_task = db_tasks::get_next(pool, agent_id, None)?;
    Ok(serde_json::json!({
        "completed": id,
        "next_task": next_task,
    }))
}

/// Validate a [`TaskStatus`] transition, returning an error for illegal moves.
///
/// Allowed transitions:
///
/// | From        | To (allowed)                           |
/// |-------------|----------------------------------------|
/// | Pending     | `InProgress`, Cancelled, Blocked         |
/// | `InProgress`  | Done, Blocked, Cancelled, Pending      |
/// | Blocked     | Pending, Cancelled                     |
/// | Done        | *(none — terminal)*                    |
/// | Cancelled   | *(none — terminal)*                    |
///
/// A self-transition (`from == to`) is always allowed (idempotent update).
fn validate_transition(
    from: hive_core::types::TaskStatus,
    to: hive_core::types::TaskStatus,
) -> Result<()> {
    use hive_core::types::TaskStatus::*;
    let allowed = match from {
        Pending     => &[InProgress, Cancelled, Blocked][..],
        InProgress  => &[Done, Blocked, Cancelled, Pending][..], // Pending = operator reset
        Blocked     => &[Pending, Cancelled][..],
        Done | Cancelled => &[][..],
    };
    if allowed.contains(&to) || from == to {
        Ok(())
    } else {
        anyhow::bail!("invalid status transition: {from} → {to}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::types::TaskStatus::*;
    use crate::db;
    use serde_json::json;

    fn open_test_db() -> crate::db::DbPool {
        let pool = db::open(":memory:").unwrap();
        db::run_migrations(&pool).unwrap();
        pool
    }

    // ── validate_transition ─────────────────────────────────────────────────

    #[test]
    fn valid_transitions_allowed() {
        assert!(validate_transition(Pending, InProgress).is_ok());
        assert!(validate_transition(Pending, Cancelled).is_ok());
        assert!(validate_transition(Pending, Blocked).is_ok());
        assert!(validate_transition(InProgress, Done).is_ok());
        assert!(validate_transition(InProgress, Blocked).is_ok());
        assert!(validate_transition(InProgress, Cancelled).is_ok());
        assert!(validate_transition(InProgress, Pending).is_ok());
        assert!(validate_transition(Blocked, Pending).is_ok());
        assert!(validate_transition(Blocked, Cancelled).is_ok());
    }

    #[test]
    fn self_transitions_always_allowed() {
        for s in [Pending, InProgress, Blocked, Done, Cancelled] {
            assert!(validate_transition(s, s).is_ok(), "{s} → {s} should be ok");
        }
    }

    #[test]
    fn terminal_states_reject_all_transitions() {
        for from in [Done, Cancelled] {
            for to in [Pending, InProgress, Blocked] {
                assert!(validate_transition(from, to).is_err(), "{from} → {to} should be rejected");
            }
        }
        assert!(validate_transition(Done, Cancelled).is_err());
        assert!(validate_transition(Cancelled, Done).is_err());
    }

    #[test]
    fn pending_to_done_rejected() {
        assert!(validate_transition(Pending, Done).is_err());
    }

    #[test]
    fn blocked_to_in_progress_rejected() {
        assert!(validate_transition(Blocked, InProgress).is_err());
    }

    #[test]
    fn blocked_to_done_rejected() {
        assert!(validate_transition(Blocked, Done).is_err());
    }

    // ── create handler ──────────────────────────────────────────────────────

    #[test]
    fn create_with_valid_title() {
        let pool = open_test_db();
        let result = create(&pool, Some(json!({"title": "My Task"}))).unwrap();
        assert_eq!(result["title"], "My Task");
        assert_eq!(result["status"], "pending");
    }

    #[test]
    fn create_with_tags_and_description() {
        let pool = open_test_db();
        let result = create(&pool, Some(json!({
            "title": "Tagged",
            "description": "A task",
            "tags": ["rust", "test"]
        }))).unwrap();
        assert_eq!(result["description"], "A task");
        let tags: Vec<&str> = result["tags"].as_array().unwrap()
            .iter().map(|t| t.as_str().unwrap()).collect();
        assert!(tags.contains(&"rust"));
        assert!(tags.contains(&"test"));
    }

    #[test]
    fn create_empty_title_rejected() {
        let pool = open_test_db();
        assert!(create(&pool, Some(json!({"title": ""}))).is_err());
    }

    #[test]
    fn create_whitespace_title_rejected() {
        let pool = open_test_db();
        assert!(create(&pool, Some(json!({"title": "   "}))).is_err());
    }

    #[test]
    fn create_missing_title_rejected() {
        let pool = open_test_db();
        assert!(create(&pool, Some(json!({}))).is_err());
    }

    #[test]
    fn create_null_params_rejected() {
        let pool = open_test_db();
        assert!(create(&pool, None).is_err());
    }

    // ── get handler ─────────────────────────────────────────────────────────

    #[test]
    fn get_existing_task() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "Find Me"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        let result = get(&pool, Some(json!({"id": id}))).unwrap();
        assert_eq!(result["title"], "Find Me");
    }

    #[test]
    fn get_unknown_id_errors() {
        let pool = open_test_db();
        assert!(get(&pool, Some(json!({"id": "nonexistent"}))).is_err());
    }

    #[test]
    fn get_missing_id_param_errors() {
        let pool = open_test_db();
        assert!(get(&pool, Some(json!({}))).is_err());
    }

    // ── list handler ────────────────────────────────────────────────────────

    #[test]
    fn list_returns_all_tasks() {
        let pool = open_test_db();
        create(&pool, Some(json!({"title": "T1"}))).unwrap();
        create(&pool, Some(json!({"title": "T2"}))).unwrap();
        let result = list(&pool, None).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[test]
    fn list_filter_by_status() {
        let pool = open_test_db();
        create(&pool, Some(json!({"title": "T1"}))).unwrap();
        let result = list(&pool, Some(json!({"status": "pending"}))).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
        let result = list(&pool, Some(json!({"status": "in-progress"}))).unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn list_filter_by_tag() {
        let pool = open_test_db();
        create(&pool, Some(json!({"title": "Rust", "tags": ["rust"]}))).unwrap();
        create(&pool, Some(json!({"title": "Python", "tags": ["python"]}))).unwrap();
        let result = list(&pool, Some(json!({"tag": "rust"}))).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["title"], "Rust");
    }

    // ── complete handler ────────────────────────────────────────────────────

    #[test]
    fn complete_returns_completed_id_and_next_task() {
        let pool = open_test_db();
        create(&pool, Some(json!({"title": "First"}))).unwrap();
        create(&pool, Some(json!({"title": "Second"}))).unwrap();
        let claimed = get_next(&pool, "agent-1", None).unwrap();
        let id = claimed["id"].as_str().unwrap();
        let result = complete(&pool, "agent-1", Some(json!({"id": id, "result": "done!"}))).unwrap();
        assert_eq!(result["completed"].as_str().unwrap(), id);
        assert!(result["next_task"].is_object());
        assert_eq!(result["next_task"]["title"], "Second");
    }

    #[test]
    fn complete_missing_id_errors() {
        let pool = open_test_db();
        assert!(complete(&pool, "agent-1", Some(json!({}))).is_err());
    }

    #[test]
    fn complete_unknown_id_errors() {
        let pool = open_test_db();
        assert!(complete(&pool, "agent-1", Some(json!({"id": "ghost"}))).is_err());
    }

    // ── update handler ──────────────────────────────────────────────────────

    #[test]
    fn update_description_and_tags() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        let result = update(&pool, Some(json!({"id": id, "description": "New desc", "tags": ["a"]}))).unwrap();
        assert_eq!(result["description"], "New desc");
    }

    #[test]
    fn update_invalid_status_transition_errors() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(update(&pool, Some(json!({"id": id, "status": "done"}))).is_err());
    }

    #[test]
    fn update_unknown_status_errors() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(update(&pool, Some(json!({"id": id, "status": "flying"}))).is_err());
    }

    #[test]
    fn update_reset_to_pending_clears_assignment() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        get_next(&pool, "agent-1", None).unwrap();
        let result = update(&pool, Some(json!({"id": id, "status": "pending"}))).unwrap();
        assert_eq!(result["status"], "pending");
        assert!(result["assigned_agent_id"].is_null());
    }

    // ── get_next handler ────────────────────────────────────────────────────

    #[test]
    fn get_next_returns_null_when_empty() {
        let pool = open_test_db();
        let result = get_next(&pool, "agent-1", None).unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn get_next_with_tag_filter() {
        let pool = open_test_db();
        create(&pool, Some(json!({"title": "Rust", "tags": ["rust"]}))).unwrap();
        let result = get_next(&pool, "agent-1", Some(json!({"tag": "python"}))).unwrap();
        assert!(result.is_null());
    }

    // ── split handler ───────────────────────────────────────────────────────

    #[test]
    fn split_missing_id_errors() {
        let pool = open_test_db();
        assert!(split(&pool, Some(json!({"subtasks": ["S1"]}))).is_err());
    }

    #[test]
    fn split_missing_subtasks_errors() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(split(&pool, Some(json!({"id": id}))).is_err());
    }

    // ── set_dependency handler ──────────────────────────────────────────────

    #[test]
    fn set_dependency_missing_task_id_errors() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(set_dependency(&pool, Some(json!({"depends_on_id": id}))).is_err());
    }

    #[test]
    fn set_dependency_missing_depends_on_id_errors() {
        let pool = open_test_db();
        let created = create(&pool, Some(json!({"title": "T"}))).unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(set_dependency(&pool, Some(json!({"task_id": id}))).is_err());
    }
}
