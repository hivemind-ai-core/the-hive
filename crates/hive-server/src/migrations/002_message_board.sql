CREATE TABLE topics (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    content          TEXT NOT NULL,
    creator_agent_id TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    last_updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE comments (
    id               TEXT PRIMARY KEY,
    topic_id         TEXT NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    content          TEXT NOT NULL,
    creator_agent_id TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
