//! Common test utilities and fixtures
//!
//! Provides shared setup for integration tests including:
//! - Test database creation
//! - Test fixtures
//! - Helper functions

use rscholar::db::{init_pool, DbConfig, DbPool};
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

/// Initialize test environment (logging, etc.)
pub fn init_test_env() {
    INIT.call_once(|| {
        // Set up minimal logging for tests
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_test_writer()
            .try_init();
    });
}

/// Test database context
/// 
/// Creates a temporary SQLite database that is automatically
/// cleaned up when this struct is dropped.
pub struct TestDb {
    pub pool: DbPool,
    _temp_dir: TempDir,
}

impl TestDb {
    /// Create a new test database with all tables initialized
    pub fn new() -> Self {
        init_test_env();
        
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");
        
        let config = DbConfig {
            path: db_path.to_string_lossy().to_string(),
            max_connections: 2,
            pool_timeout_secs: 5,
            busy_timeout_ms: 1000,
        };
        
        let pool = init_pool(&config).expect("Failed to init test database");
        
        Self {
            pool,
            _temp_dir: temp_dir,
        }
    }
    
    /// Get a sync connection for direct DB operations
    pub fn get_conn(&self) -> rusqlite::Connection {
        let path = self._temp_dir.path().join("test.db");
        rusqlite::Connection::open(path).expect("Failed to open connection")
    }
}

impl Default for TestDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Macro to create a test database and run async code
#[macro_export]
macro_rules! with_test_db {
    ($db:ident, $body:block) => {{
        let $db = common::TestDb::new();
        $body
    }};
}
