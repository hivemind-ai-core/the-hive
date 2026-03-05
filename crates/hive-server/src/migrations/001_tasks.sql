CREATE TABLE tasks (
    id                TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    description       TEXT,
    status            TEXT NOT NULL DEFAULT 'pending',
    assigned_agent_id TEXT,
    tags              TEXT NOT NULL DEFAULT '[]',  -- JSON array
    result            TEXT,
    position          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE task_dependencies (
    task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_id)
);
