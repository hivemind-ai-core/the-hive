//! Common error types for The Hive.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),
}
