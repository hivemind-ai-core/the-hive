//! Database connection management.
//!
//! Currently backed by a single `Arc<Mutex<Connection>>`. The `DbPool` type
//! and `get()` interface are designed so the backing implementation can be
//! swapped (e.g. to r2d2) without changing call sites.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A cloneable handle to the database. Cheap to clone.
#[derive(Clone)]
pub struct DbPool(Arc<Mutex<Connection>>);

impl DbPool {
    /// Acquire the database connection.
    pub fn get(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0.lock().map_err(|_| anyhow::anyhow!("DB lock poisoned"))
    }
}

/// Open (or create) the database at `db_path` and return a pool handle.
pub fn open(db_path: &str) -> Result<DbPool> {
    if let Some(parent) = Path::new(db_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating DB directory {:?}", parent))?;
        }
    }

    let conn = Connection::open(db_path)
        .with_context(|| format!("opening database at {db_path}"))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .context("setting PRAGMAs")?;

    Ok(DbPool(Arc::new(Mutex::new(conn))))
}

/// Run all pending schema migrations.
pub fn run_migrations(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("creating schema_migrations table")?;

    for (version, sql) in MIGRATIONS {
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM schema_migrations WHERE version = ?1",
                rusqlite::params![version],
                |row| row.get(0),
            )
            .context("checking migration version")?;

        if !already_applied {
            conn.execute_batch(sql)
                .with_context(|| format!("applying migration v{version}"))?;
            conn.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                rusqlite::params![version],
            )
            .context("recording migration")?;
            tracing::info!("Applied migration v{}", version);
        }
    }

    Ok(())
}

/// Ordered list of schema migrations. Each entry is `(version, sql)`.
/// New migrations are appended here as tables are introduced.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/001_tasks.sql")),
    (2, include_str!("migrations/002_message_board.sql")),
    (3, include_str!("migrations/003_communication.sql")),
    (4, include_str!("migrations/004_indexes.sql")),
];

#[cfg(test)]
pub(crate) fn open_test_db() -> DbPool {
    let pool = open(":memory:").expect("open in-memory db");
    run_migrations(&pool).expect("run migrations");
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_succeeds() {
        let pool = open(":memory:").expect("should open");
        let conn = pool.get().expect("should get connection");
        // :memory: DB returns "memory" even with WAL pragma — that is expected.
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert!(mode == "wal" || mode == "memory");
    }

    #[test]
    fn run_migrations_creates_all_tables() {
        let pool = open_test_db();
        let conn = pool.get().unwrap();
        for table in &[
            "tasks",
            "task_dependencies",
            "topics",
            "comments",
            "push_messages",
            "agents",
            "schema_migrations",
        ] {
            let count: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{table}'"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table '{table}' should exist after migrations");
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let pool = open_test_db();
        // Running again must not error.
        run_migrations(&pool).expect("second migration run should succeed");
    }

    #[test]
    fn migration_versions_all_recorded() {
        let pool = open_test_db();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count,
            MIGRATIONS.len() as i64,
            "every migration should be recorded exactly once"
        );
    }
}
