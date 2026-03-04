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
}

/// API request/response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub id: String,
    pub method: Option<String>,
    pub params: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Request,
    Response,
    Error,
    Push,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
}
