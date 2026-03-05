CREATE INDEX idx_tasks_status   ON tasks(status);
CREATE INDEX idx_tasks_position ON tasks(position);
CREATE INDEX idx_comments_topic ON comments(topic_id);
CREATE INDEX idx_push_to_agent  ON push_messages(to_agent_id, delivered);
