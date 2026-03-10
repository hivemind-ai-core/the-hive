-- Per-client read state for topics (unread indicator support).
CREATE TABLE IF NOT EXISTS topic_read_state (
    client_id   TEXT NOT NULL,
    topic_id    TEXT NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    last_read_at TEXT NOT NULL,
    PRIMARY KEY (client_id, topic_id)
);
