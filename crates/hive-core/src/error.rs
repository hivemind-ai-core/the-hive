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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        assert_eq!(
            Error::Database("connection refused".to_string()).to_string(),
            "Database error: connection refused"
        );
        assert_eq!(
            Error::Network("timeout".to_string()).to_string(),
            "Network error: timeout"
        );
        assert_eq!(
            Error::Config("missing key".to_string()).to_string(),
            "Configuration error: missing key"
        );
        assert_eq!(
            Error::Agent("crashed".to_string()).to_string(),
            "Agent error: crashed"
        );
        assert_eq!(
            Error::TaskNotFound("task-123".to_string()).to_string(),
            "Task not found: task-123"
        );
        assert_eq!(
            Error::TopicNotFound("topic-456".to_string()).to_string(),
            "Topic not found: topic-456"
        );
        assert_eq!(
            Error::AgentNotFound("agent-789".to_string()).to_string(),
            "Agent not found: agent-789"
        );
    }

    #[test]
    fn error_from_serde_json() {
        let bad_json = serde_json::from_str::<serde_json::Value>("{bad}");
        assert!(bad_json.is_err());
        let err = Error::Serialization(bad_json.unwrap_err());
        assert!(err.to_string().starts_with("Serialization error:"));
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = Error::Io(io_err);
        assert!(err.to_string().starts_with("IO error:"));
        assert!(err.to_string().contains("file missing"));
    }
}
