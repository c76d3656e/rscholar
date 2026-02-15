//! Database schema initialization.
//!
//! Creates all required tables for the application.

use crate::error::{GscholarError, Result};
use rusqlite::Connection;
use tracing::debug;

/// SQL statements for table creation
const SCHEMA_SQL: &str = r#"
-- Tasks table (persistent task queue)
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'pending',
    progress_step TEXT DEFAULT 'Initializing',
    progress_percent INTEGER DEFAULT 0,
    result_json TEXT,
    error TEXT,
    keyword TEXT,
    source TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at);

-- API Keys table
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    key_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    is_admin INTEGER DEFAULT 0,
    rate_limit_rps INTEGER DEFAULT 10,
    request_count INTEGER DEFAULT 0,
    last_used_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

-- Journal cache table
CREATE TABLE IF NOT EXISTS journal_cache (
    name TEXT PRIMARY KEY,
    sciif TEXT,
    jci TEXT,
    sci TEXT,
    sci_up_top TEXT,
    sci_base TEXT,
    sci_up TEXT,
    fetched_at INTEGER NOT NULL
);

-- Search logs table
CREATE TABLE IF NOT EXISTS search_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id TEXT,
    keyword TEXT NOT NULL,
    source TEXT,
    result_count INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_search_logs_keyword ON search_logs(keyword);
CREATE INDEX IF NOT EXISTS idx_search_logs_created ON search_logs(created_at);

-- Journal hits table (for statistics)
CREATE TABLE IF NOT EXISTS journal_hits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    search_id INTEGER REFERENCES search_logs(id) ON DELETE CASCADE,
    journal_name TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_journal_hits_name ON journal_hits(journal_name);

-- Cache stats table (for tracking hit rates)
CREATE TABLE IF NOT EXISTS cache_stats (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    total_lookups INTEGER DEFAULT 0,
    cache_hits INTEGER DEFAULT 0
);

INSERT OR IGNORE INTO cache_stats (id, total_lookups, cache_hits) VALUES (1, 0, 0);
"#;

/// Initialize all database tables
///
/// This function is idempotent - safe to call multiple times.
/// Also enables WAL mode for better concurrent read/write performance.
pub fn init_tables(conn: &Connection) -> Result<()> {
    debug!("Initializing database schema");
    
    // Enable WAL mode for better concurrent read/write performance
    // Set busy_timeout to prevent SQLITE_BUSY errors under load
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         PRAGMA synchronous=NORMAL;"
    ).map_err(|e| GscholarError::Database(format!("Failed to set pragmas: {}", e)))?;
    
    conn.execute_batch(SCHEMA_SQL)
        .map_err(|e| GscholarError::Database(format!("Failed to init schema: {}", e)))?;
    
    debug!("Database schema initialized with WAL mode");
    Ok(())
}

/// Get database statistics
pub fn get_db_stats(conn: &Connection) -> Result<DbStats> {
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap_or(0);
    
    let key_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM api_keys", [], |row| row.get(0))
        .unwrap_or(0);
    
    let cache_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM journal_cache", [], |row| row.get(0))
        .unwrap_or(0);
    
    let search_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM search_logs", [], |row| row.get(0))
        .unwrap_or(0);

    Ok(DbStats {
        task_count,
        key_count,
        cache_count,
        search_count,
    })
}

/// Database statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct DbStats {
    pub task_count: i64,
    pub key_count: i64,
    pub cache_count: i64,
    pub search_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tables() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        assert!(init_tables(&conn).is_ok());
        
        // Verify tables exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        
        assert!(count >= 5); // At least 5 tables
    }

    #[test]
    fn test_get_db_stats() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        init_tables(&conn).expect("init");
        
        let stats = get_db_stats(&conn).expect("stats");
        assert_eq!(stats.task_count, 0);
        assert_eq!(stats.key_count, 0);
    }
}
