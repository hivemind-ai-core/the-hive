//! Common types used across The Hive components.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in-progress"),
            TaskStatus::Done => write!(f, "done"),
            TaskStatus::Blocked => write!(f, "blocked"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A task in the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub assigned_agent_id: Option<String>,
    pub tags: Vec<String>,
    pub result: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub position: i32,
}

impl Task {
    #[must_use]
    pub fn new(title: String, description: Option<String>, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            description,
            status: TaskStatus::Pending,
            assigned_agent_id: None,
            tags,
            result: None,
            created_at: now,
            updated_at: now,
            position: 0,
        }
    }
}

/// A topic in the message board
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub content: String,
    pub creator_agent_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_updated_at: DateTime<Utc>,
}

impl Topic {
    #[must_use]
    pub fn new(title: String, content: String, creator_agent_id: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            content,
            creator_agent_id,
            created_at: now,
            last_updated_at: now,
        }
    }
}

/// A comment on a topic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub topic_id: String,
    pub content: String,
    pub creator_agent_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Comment {
    #[must_use]
    pub fn new(topic_id: String, content: String, creator_agent_id: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            topic_id,
            content,
            creator_agent_id,
            created_at: Utc::now(),
        }
    }
}

/// A push message between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushMessage {
    pub id: String,
    pub from_agent_id: Option<String>,
    pub to_agent_id: String,
    pub content: String,
    pub delivered: bool,
    pub created_at: DateTime<Utc>,
}

impl PushMessage {
    #[must_use]
    pub fn new(to_agent_id: String, content: String, from_agent_id: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            from_agent_id,
            to_agent_id,
            content,
            delivered: false,
            created_at: Utc::now(),
        }
    }
}

/// A registered agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Maximum number of concurrent tasks this agent can handle.
    /// Defaults to 1. Server assigns work when `active_tasks` < `capacity_max`.
    #[serde(default = "default_capacity_max")]
    pub capacity_max: u8,
}

fn default_capacity_max() -> u8 { 1 }

#[cfg(test)]
mod tests {
    use super::*;

    // ── Task ─────────────────────────────────────────────────────────────────

    #[test]
    fn task_new_defaults() {
        let task = Task::new("My Task".to_string(), None, vec![]);
        assert!(!task.id.is_empty(), "id should be a non-empty UUID");
        assert_eq!(task.title, "My Task");
        assert!(task.description.is_none());
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.assigned_agent_id.is_none());
        assert!(task.tags.is_empty());
        assert!(task.result.is_none());
        assert_eq!(task.position, 0);
    }

    #[test]
    fn task_new_with_all_fields() {
        let tags = vec!["rust".to_string(), "backend".to_string()];
        let task = Task::new(
            "Full Task".to_string(),
            Some("A description".to_string()),
            tags.clone(),
        );
        assert_eq!(task.description.as_deref(), Some("A description"));
        assert_eq!(task.tags, tags);
    }

    #[test]
    fn task_new_generates_unique_ids() {
        let t1 = Task::new("T1".to_string(), None, vec![]);
        let t2 = Task::new("T2".to_string(), None, vec![]);
        assert_ne!(t1.id, t2.id);
    }

    #[test]
    fn task_status_serde_kebab_case() {
        // Serializes to kebab-case strings.
        assert_eq!(
            serde_json::to_string(&TaskStatus::InProgress).unwrap(),
            "\"in-progress\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Done).unwrap(),
            "\"done\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Blocked).unwrap(),
            "\"blocked\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn task_status_deserialize_kebab_case() {
        let s: TaskStatus = serde_json::from_str("\"in-progress\"").unwrap();
        assert_eq!(s, TaskStatus::InProgress);
        let s: TaskStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(s, TaskStatus::Pending);
    }

    #[test]
    fn task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::InProgress.to_string(), "in-progress");
        assert_eq!(TaskStatus::Done.to_string(), "done");
        assert_eq!(TaskStatus::Blocked.to_string(), "blocked");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn task_status_deserialize_unknown_returns_error() {
        let result: Result<TaskStatus, _> = serde_json::from_str("\"unknown\"");
        assert!(result.is_err());
    }

    #[test]
    fn task_roundtrip_json() {
        let task = Task::new("Round Trip".to_string(), Some("desc".to_string()), vec!["tag1".to_string()]);
        let json = serde_json::to_string(&task).unwrap();
        let decoded: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, task.id);
        assert_eq!(decoded.title, task.title);
        assert_eq!(decoded.description, task.description);
        assert_eq!(decoded.status, task.status);
        assert_eq!(decoded.tags, task.tags);
    }

    // ── Topic ─────────────────────────────────────────────────────────────────

    #[test]
    fn topic_new_defaults() {
        let topic = Topic::new("My Topic".to_string(), "Content here".to_string(), Some("agent-1".to_string()));
        assert!(!topic.id.is_empty());
        assert_eq!(topic.title, "My Topic");
        assert_eq!(topic.content, "Content here");
        assert_eq!(topic.creator_agent_id.as_deref(), Some("agent-1"));
    }

    #[test]
    fn topic_new_without_creator() {
        let topic = Topic::new("Anon Topic".to_string(), "Body".to_string(), None);
        assert!(topic.creator_agent_id.is_none());
    }

    #[test]
    fn topic_new_timestamps_equal() {
        let topic = Topic::new("T".to_string(), "C".to_string(), None);
        // created_at and last_updated_at are both set to now() on construction.
        assert!(topic.last_updated_at >= topic.created_at);
    }

    #[test]
    fn topic_roundtrip_json() {
        let topic = Topic::new("Round".to_string(), "Trip".to_string(), Some("a".to_string()));
        let json = serde_json::to_string(&topic).unwrap();
        let decoded: Topic = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, topic.id);
        assert_eq!(decoded.title, topic.title);
        assert_eq!(decoded.content, topic.content);
    }

    // ── Comment ───────────────────────────────────────────────────────────────

    #[test]
    fn comment_new_defaults() {
        let c = Comment::new("topic-1".to_string(), "Hello".to_string(), Some("agent-2".to_string()));
        assert!(!c.id.is_empty());
        assert_eq!(c.topic_id, "topic-1");
        assert_eq!(c.content, "Hello");
        assert_eq!(c.creator_agent_id.as_deref(), Some("agent-2"));
    }

    #[test]
    fn comment_new_without_creator() {
        let c = Comment::new("t".to_string(), "C".to_string(), None);
        assert!(c.creator_agent_id.is_none());
    }

    #[test]
    fn comment_roundtrip_json() {
        let c = Comment::new("tid".to_string(), "body".to_string(), None);
        let json = serde_json::to_string(&c).unwrap();
        let decoded: Comment = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, c.id);
        assert_eq!(decoded.topic_id, c.topic_id);
        assert_eq!(decoded.content, c.content);
    }

    // ── PushMessage ───────────────────────────────────────────────────────────

    #[test]
    fn push_message_new_defaults() {
        let m = PushMessage::new("agent-b".to_string(), "ping".to_string(), Some("agent-a".to_string()));
        assert!(!m.id.is_empty());
        assert_eq!(m.to_agent_id, "agent-b");
        assert_eq!(m.content, "ping");
        assert_eq!(m.from_agent_id.as_deref(), Some("agent-a"));
        assert!(!m.delivered, "new messages should not be delivered");
    }

    #[test]
    fn push_message_new_no_sender() {
        let m = PushMessage::new("agent-b".to_string(), "msg".to_string(), None);
        assert!(m.from_agent_id.is_none());
    }

    #[test]
    fn push_message_roundtrip_json() {
        let m = PushMessage::new("b".to_string(), "text".to_string(), Some("a".to_string()));
        let json = serde_json::to_string(&m).unwrap();
        let decoded: PushMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, m.id);
        assert_eq!(decoded.delivered, false);
    }

    // ── Agent ─────────────────────────────────────────────────────────────────

    #[test]
    fn agent_capacity_max_defaults_to_1() {
        let json = r#"{"id":"a","name":"n","tags":[]}"#;
        let agent: Agent = serde_json::from_str(json).unwrap();
        assert_eq!(agent.capacity_max, 1);
    }

    #[test]
    fn agent_capacity_max_explicit() {
        let json = r#"{"id":"a","name":"n","tags":[],"capacity_max":4}"#;
        let agent: Agent = serde_json::from_str(json).unwrap();
        assert_eq!(agent.capacity_max, 4);
    }

    // ── ApiMessage / MessageType ──────────────────────────────────────────────

    #[test]
    fn message_type_serde() {
        assert_eq!(serde_json::to_string(&MessageType::Request).unwrap(), "\"request\"");
        assert_eq!(serde_json::to_string(&MessageType::Response).unwrap(), "\"response\"");
        assert_eq!(serde_json::to_string(&MessageType::Push).unwrap(), "\"push\"");
        assert_eq!(serde_json::to_string(&MessageType::Error).unwrap(), "\"error\"");
    }

    #[test]
    fn api_message_roundtrip_json() {
        let msg = ApiMessage {
            msg_type: MessageType::Request,
            id: "req-1".to_string(),
            method: Some("task.create".to_string()),
            params: Some(serde_json::json!({"title": "T"})),
            result: None,
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: ApiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "req-1");
        assert_eq!(decoded.method.as_deref(), Some("task.create"));
        assert!(decoded.result.is_none());
        assert!(decoded.error.is_none());
    }

    #[test]
    fn api_error_serde() {
        let err = ApiError { code: 404, message: "not found".to_string() };
        let json = serde_json::to_string(&err).unwrap();
        let decoded: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.code, 404);
        assert_eq!(decoded.message, "not found");
    }
}

/// JSON-RPC–like message envelope used for all WebSocket communication.
///
/// - **Request**: `type="request"`, `method` and `params` are set; `id` is a UUID
///   the caller uses to match the response.
/// - **Response**: `type="response"`, `id` echoes the request id; `result` holds the value.
/// - **Error**: `type="error"`, `id` echoes the request id; `error` describes the failure.
/// - **Push**: `type="push"`, server-initiated; `method` names the event (e.g. `task.assign`);
///   `id` is a fresh UUID. No response is expected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// Correlation ID. Callers set this on requests; servers echo it on responses/errors.
    pub id: String,
    /// Method name (e.g. `"task.create"`). Present on requests and push messages.
    pub method: Option<String>,
    /// Request parameters. Present on requests only.
    pub params: Option<serde_json::Value>,
    /// Success payload. Present on response messages only.
    pub result: Option<serde_json::Value>,
    /// Error detail. Present on error messages only.
    pub error: Option<ApiError>,
}

/// Discriminator for the [`ApiMessage`] envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    /// Client-initiated call expecting a response.
    Request,
    /// Server reply to a request (success).
    Response,
    /// Server reply to a request (failure).
    Error,
    /// Server-initiated notification. No response expected.
    Push,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
}
