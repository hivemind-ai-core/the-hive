CREATE TABLE agents (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    tags         TEXT NOT NULL DEFAULT '[]',  -- JSON array
    connected_at TEXT,
    last_seen_at TEXT
);

CREATE TABLE push_messages (
    id            TEXT PRIMARY KEY,
    from_agent_id TEXT,
    to_agent_id   TEXT NOT NULL,
    content       TEXT NOT NULL,
    delivered     INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
