//! hive-server library — exposes internal modules for integration tests.

pub mod agent_registry;
pub mod communication;
pub mod db;
pub mod handlers;
pub mod message_board;
pub mod state;
pub mod tasks;
pub mod ws;

#[cfg(test)]
pub mod test_helpers {
    //! Shared test-only factory helpers used across hive-server unit test modules.
    use chrono::Utc;
    use hive_core::types::{Agent, Comment, PushMessage, Task, Topic};

    pub fn make_task(title: &str) -> Task {
        Task::new(title.to_string(), None, vec![])
    }

    pub fn make_tagged_task(title: &str, tags: &[&str]) -> Task {
        Task::new(
            title.to_string(),
            None,
            tags.iter().map(|s| s.to_string()).collect(),
        )
    }

    pub fn make_topic(title: &str) -> Topic {
        Topic::new(title.to_string(), "content".to_string(), Some("agent-1".to_string()))
    }

    pub fn make_comment(topic_id: &str, content: &str) -> Comment {
        Comment::new(topic_id.to_string(), content.to_string(), Some("agent-1".to_string()))
    }

    pub fn make_agent(id: &str) -> Agent {
        Agent {
            id: id.to_string(),
            name: format!("Agent {id}"),
            tags: vec![],
            connected_at: Some(Utc::now()),
            last_seen_at: Some(Utc::now()),
            capacity_max: 1,
        }
    }

    pub fn make_msg(to: &str, from: &str, content: &str) -> PushMessage {
        PushMessage::new(to.to_string(), content.to_string(), Some(from.to_string()))
    }
}
