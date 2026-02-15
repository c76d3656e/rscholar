//! Database module with async-safe SQLite persistence.
//!
//! Uses deadpool-sqlite for async connection pooling with:
//! - WAL mode (better concurrent reads)
//! - busy_timeout (prevents immediate failures under contention)
//! - Connection timeout (pool.get() doesn't wait forever)
//!
//! Provides CRUD operations for:
//! - Tasks (persistent task queue)
//! - API Keys (dynamic key management)
//! - Journal Cache (EasyScholar caching)
//! - Analytics (usage statistics)

pub mod analytics;
pub mod api_keys;
pub mod journal_cache;
pub mod schema;
pub mod tasks;

use crate::error::{GscholarError, Result};
use deadpool_sqlite::{Config, Pool, Runtime};
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;
use tracing::info;

/// Type alias for the async connection pool
pub type DbPool = Pool;

/// Type alias for a connection (used in closures)
pub type DbConn = Connection;

/// Database configuration
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Path to SQLite database file
    pub path: String,
    /// Maximum number of connections in pool (keep small for SQLite)
    pub max_connections: usize,
    /// Timeout waiting for a connection from pool
    pub pool_timeout_secs: u64,
    /// SQLite busy timeout (ms) - how long to wait when DB is locked
    pub busy_timeout_ms: u32,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            path: "data/rscholar.db".to_string(),
            max_connections: 4,        // SQLite prefers small pools
            pool_timeout_secs: 5,      // Don't wait forever for connection
            busy_timeout_ms: 5000,     // 5 seconds for busy timeout
        }
    }
}

/// Initialize database connection pool
///
/// Creates the database file and parent directories if they don't exist.
/// Configures WAL mode and busy timeout for production use.
///
/// # Example
/// ```no_run
/// use rscholar::db::{init_pool, DbConfig};
///
/// #[tokio::main]
/// async fn main() {
///     let config = DbConfig::default();
///     let pool = init_pool(&config).expect("Failed to init database");
/// }
/// ```
pub fn init_pool(config: &DbConfig) -> Result<DbPool> {
    // Create parent directory if needed
    if let Some(parent) = Path::new(&config.path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    info!(path = %config.path, max_conn = config.max_connections, "Initializing database");

    // Create deadpool config
    let cfg = Config::new(&config.path);
    
    let pool = cfg.builder(Runtime::Tokio1)
        .map_err(|e| GscholarError::Database(format!("Failed to create pool builder: {}", e)))?
        .max_size(config.max_connections)
        .wait_timeout(Some(Duration::from_secs(config.pool_timeout_secs)))
        .build()
        .map_err(|e| GscholarError::Database(format!("Failed to create pool: {}", e)))?;

    // Initialize schema synchronously (one-time setup)
    let busy_timeout = config.busy_timeout_ms;
    {
        let conn = Connection::open(&config.path)
            .map_err(|e| GscholarError::Database(format!("Failed to open database: {}", e)))?;
        
        // Configure SQLite for production
        configure_connection(&conn, busy_timeout)?;
        
        // Initialize tables
        schema::init_tables(&conn)?;
    }

    info!("Database initialized with WAL mode");
    Ok(pool)
}

/// Configure a SQLite connection for production use
pub fn configure_connection(conn: &Connection, busy_timeout_ms: u32) -> Result<()> {
    // WAL mode for better concurrent read performance
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| GscholarError::Database(format!("Failed to set WAL mode: {}", e)))?;
    
    // Busy timeout - wait instead of failing immediately when locked
    conn.pragma_update(None, "busy_timeout", busy_timeout_ms)
        .map_err(|e| GscholarError::Database(format!("Failed to set busy_timeout: {}", e)))?;
    
    // Synchronous mode for durability (NORMAL is a good balance)
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| GscholarError::Database(format!("Failed to set synchronous: {}", e)))?;
    
    // Cache size (negative = KB, so -2000 = 2MB)
    conn.pragma_update(None, "cache_size", -2000i32)
        .map_err(|e| GscholarError::Database(format!("Failed to set cache_size: {}", e)))?;
    
    Ok(())
}

/// Get a connection from the pool (async)
///
/// Returns an error if timeout expires waiting for a connection.
pub async fn get_conn_async(pool: &DbPool, busy_timeout_ms: u32) -> Result<deadpool_sqlite::Object> {
    let obj = pool.get().await
        .map_err(|e| GscholarError::Database(format!("Pool timeout: {}", e)))?;
    
    // Configure connection on first use
    obj.interact(move |conn| {
        configure_connection(conn, busy_timeout_ms)
    }).await
    .map_err(|e| GscholarError::Database(format!("Connection config failed: {}", e)))??;
    
    Ok(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_pool() {
        let tmp = TempDir::new().expect("temp dir");
        let config = DbConfig {
            path: tmp.path().join("test.db").to_string_lossy().to_string(),
            max_connections: 2,
            pool_timeout_secs: 5,
            busy_timeout_ms: 1000,
        };
        
        let pool = init_pool(&config);
        assert!(pool.is_ok());
    }

    #[test]
    fn test_wal_mode() {
        let tmp = TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("wal_test.db");
        
        let conn = Connection::open(&db_path).expect("open");
        configure_connection(&conn, 5000).expect("configure");
        
        // Verify WAL mode
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("query");
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
