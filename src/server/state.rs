//! Shared application state for the HTTP server.
//!
//! Contains all shared resources accessed by handlers.
//! 
//! ## Async-Safe Database Access
//! 
//! Uses deadpool-sqlite's `interact()` method which automatically
//! runs blocking SQLite operations on a separate thread pool.

use super::config::ServerConfig;
use super::task::TaskStore;
use crate::db::DbPool;
use crate::error::{GscholarError, Result};
use std::sync::Arc;
use tracing::info;

/// Default busy timeout for SQLite connections (5 seconds)
const DEFAULT_BUSY_TIMEOUT_MS: u32 = 5000;

/// Shared application state
///
/// Thread-safe, cloneable state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    /// Server configuration
    pub config: Arc<ServerConfig>,
    /// Task storage for async jobs (in-memory)
    pub task_store: TaskStore,
    /// Database connection pool (async-safe)
    pub db: DbPool,
    /// LLM Relevance Filter (optional, global concurrency control)
    pub llm_filter: Option<Arc<crate::llm::LlmRelevanceFilter>>,
    /// Ranking microservice (optional when easyscholar keys are not configured)
    pub ranking_service: Option<Arc<crate::rankings::RankingService>>,
    /// SQLite busy timeout in ms
    pub busy_timeout_ms: u32,
}

impl AppState {
    /// Create new application state
    ///
    /// # Arguments
    /// * `config` - Server configuration
    /// * `db` - Deadpool SQLite connection pool
    pub fn new(
        config: ServerConfig, 
        db: DbPool,
        llm_filter: Option<Arc<crate::llm::LlmRelevanceFilter>>,
        ranking_service: Option<Arc<crate::rankings::RankingService>>,
    ) -> Self {
        info!(
            host = %config.server.host,
            port = config.server.port,
            llm_enabled = llm_filter.is_some(),
            ranking_service_enabled = ranking_service.is_some(),
            "Initializing AppState"
        );
        Self {
            config: Arc::new(config),
            task_store: TaskStore::new(),
            db,
            llm_filter,
            ranking_service,
            busy_timeout_ms: DEFAULT_BUSY_TIMEOUT_MS,
        }
    }

    /// Create with custom busy timeout
    pub fn with_busy_timeout(mut self, timeout_ms: u32) -> Self {
        info!(timeout_ms, "Updating SQLite busy timeout");
        self.busy_timeout_ms = timeout_ms;
        self
    }

    /// Run a database operation asynchronously.
    ///
    /// Uses deadpool-sqlite's `interact()` which runs the closure
    /// on a dedicated thread pool, then returns the result async.
    ///
    /// # Example
    /// ```ignore
    /// let keys = state.run_db(|conn| {
    ///     api_keys::list(conn, 1, 10)
    /// }).await?;
    /// ```
    pub async fn run_db<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let busy_timeout = self.busy_timeout_ms;
        
        // Get connection from pool (with timeout)
        let conn = self.db.get().await
            .map_err(|e| GscholarError::Database(format!("Pool timeout: {}", e)))?;
        
        // Run the blocking operation via interact()
        conn.interact(move |conn| {
            // Configure connection if not already done
            // SQLite pragmas are per-connection, so we set them each time
            let _ = conn.pragma_update(None, "busy_timeout", busy_timeout);
            
            // Execute the user's function
            f(conn)
        })
        .await
        .map_err(|e| GscholarError::Database(format!("DB interact error: {}", e)))?
    }
}
