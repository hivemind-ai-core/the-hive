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
        let new_status = parse_status(status_str)?;
        validate_transition(task.status, new_status)?;
        task.status = new_status;
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

fn parse_status(s: &str) -> Result<hive_core::types::TaskStatus> {
    use hive_core::types::TaskStatus::*;
    match s {
        "pending"     => Ok(Pending),
        "in-progress" => Ok(InProgress),
        "done"        => Ok(Done),
        "blocked"     => Ok(Blocked),
        "cancelled"   => Ok(Cancelled),
        other => anyhow::bail!("unknown status: {other}"),
    }
}

fn validate_transition(
    from: hive_core::types::TaskStatus,
    to: hive_core::types::TaskStatus,
) -> Result<()> {
    use hive_core::types::TaskStatus::*;
    let allowed = match from {
        Pending     => &[InProgress, Cancelled, Blocked][..],
        InProgress  => &[Done, Blocked, Cancelled][..],
        Blocked     => &[Pending, Cancelled][..],
        Done | Cancelled => &[][..],
    };
    if allowed.contains(&to) || from == to {
        Ok(())
    } else {
        anyhow::bail!("invalid status transition: {from:?} → {to:?}")
    }
}
