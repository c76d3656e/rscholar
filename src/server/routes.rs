//! Router configuration with middleware.
//!
//! Assembles all routes. Authentication and rate limiting
//! are handled by external services (e.g., Cloudflare WAF).

use super::admin;
use super::handlers::{health_handler, pipeline_handler, sources_handler, task_bibtex_handler, task_download_handler, task_status_handler};
use super::state::AppState;
use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

/// Create the API router with all middleware
pub fn create_router(state: AppState, static_dir: Option<String>) -> Router {
    let admin_enabled = state.config.server.admin_enabled;
    info!(
        has_ranking_service = state.ranking_service.is_some(),
        has_llm_filter = state.llm_filter.is_some(),
        admin_enabled = admin_enabled,
        static_dir = ?static_dir,
        "Building API router"
    );

    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Admin routes (nested under /api/v1/admin)
    let admin_routes: Router = Router::new()
        // API Keys (kept for potential future use)
        .route("/keys", get(admin::list_keys_handler))
        .route("/keys", post(admin::create_key_handler))
        .route("/keys/{id}", get(admin::get_key_handler))
        .route("/keys/{id}", patch(admin::update_key_handler))
        .route("/keys/{id}", delete(admin::delete_key_handler))
        // Cache
        .route("/cache/journals", get(admin::list_cache_handler))
        .route("/cache/journals", delete(admin::clear_cache_handler))
        .route("/cache/journals/{name}", delete(admin::delete_cache_entry_handler))
        .route("/cache/stats", get(admin::cache_stats_handler))
        // Stats
        .route("/stats/overview", get(admin::stats_overview_handler))
        .route("/stats/keywords", get(admin::top_keywords_handler))
        .route("/stats/journals", get(admin::top_journals_handler))
        .route("/stats/daily", get(admin::daily_stats_handler))
        // System
        .route("/system", get(admin::system_status_handler))
        // Apply Admin Auth Middleware to ALL admin routes
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            super::middleware::require_admin,
        ))
        .with_state(state.clone());

    // Build API router - no auth, no rate limiting (handled by Cloudflare)
    let mut api_router: Router = Router::new()
        .route("/health", get(health_handler))
        .route("/sources", get(sources_handler))
        .route("/tasks", post(pipeline_handler))
        .route("/tasks/{id}", get(task_status_handler))
        .route("/tasks/{id}/download", get(task_download_handler))
        .route("/tasks/{id}/bibtex", get(task_bibtex_handler))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    if admin_enabled {
        api_router = api_router.nest("/api/v1/admin", admin_routes);
    } else {
        info!("Admin routes are disabled by configuration");
    }

    // If static_dir is provided, serve static files as fallback (SPA mode)
    if let Some(dir) = static_dir {
        let index_path = format!("{}/index.html", dir);
        info!(static_dir = %dir, index = %index_path, "Enabled static file fallback service");
        let serve_dir = ServeDir::new(&dir)
            .not_found_service(ServeFile::new(&index_path));
        
        api_router.fallback_service(serve_dir)
    } else {
        api_router
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::config::ServerConfig;
    use crate::db::{init_pool, DbConfig};
    use tempfile::TempDir;

    #[test]
    fn test_create_router() {
        let tmp = TempDir::new().expect("temp dir");
        let db_config = DbConfig {
            path: tmp.path().join("test.db").to_string_lossy().to_string(),
            max_connections: 2,
            pool_timeout_secs: 5,
            busy_timeout_ms: 1000,
        };
        let db = init_pool(&db_config).expect("db pool");

        let config = ServerConfig::default();
        let state = AppState::new(config, db, None, None); // None for llm_filter/ranking_service in test
        let _router = create_router(state, None);
        // Router creation should not panic
    }
}
