//! Admin API handlers for management endpoints.
//!
//! All DB operations use `state.run_db()` to prevent blocking Tokio workers.

use super::responses::{self, ApiErrorResponse, Pagination};
use super::state::AppState;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_page() -> u32 { 1 }
fn default_limit() -> u32 { 20 }

#[derive(Debug, Deserialize)]
pub struct DaysQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

fn default_days() -> u32 { 7 }

// ============================================================================
// API Key Management
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default = "default_rps")]
    pub rate_limit_rps: u32,
}

fn default_rps() -> u32 { 10 }

#[derive(Debug, Deserialize)]
pub struct UpdateKeyRequest {
    pub name: Option<String>,
    pub rate_limit_rps: Option<u32>,
}

/// GET /api/v1/admin/keys - List all API keys
pub async fn list_keys_handler(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::api_keys;

    let page = query.page;
    let limit = query.limit;

    let (keys, total) = state.run_db(move |conn| {
        api_keys::list(&conn, page, limit)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to list keys");
        ApiErrorResponse::internal_error("Failed to list keys")
    })?;

    let pagination = Pagination::new(page, limit, total);
    Ok(responses::success_paginated(keys, pagination))
}

/// POST /api/v1/admin/keys - Create a new API key
pub async fn create_key_handler(
    State(state): State<AppState>,
    Json(request): Json<CreateKeyRequest>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::api_keys;

    if request.name.trim().is_empty() {
        return Err(ApiErrorResponse::bad_request("name is required"));
    }

    let name = request.name.clone();
    let is_admin = request.is_admin;
    let rps = request.rate_limit_rps;

    let created = state.run_db(move |conn| {
        api_keys::create(&conn, &name, is_admin, rps)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to create key");
        ApiErrorResponse::internal_error("Failed to create key")
    })?;

    info!(key_id = %created.id, name = %created.name, "API key created");
    Ok(responses::success(created))
}

/// GET /api/v1/admin/keys/:id - Get key details
pub async fn get_key_handler(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::api_keys;

    let id = key_id.clone();
    let key = state.run_db(move |conn| {
        api_keys::get_by_id(&conn, &id)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get key");
        ApiErrorResponse::internal_error("Failed to get key")
    })?.ok_or_else(|| ApiErrorResponse::not_found("Key not found"))?;

    Ok(responses::success(key))
}

/// PATCH /api/v1/admin/keys/:id - Update key settings
pub async fn update_key_handler(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
    Json(request): Json<UpdateKeyRequest>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::api_keys;

    let id = key_id.clone();
    let name = request.name.clone();
    let rps = request.rate_limit_rps;

    let updated = state.run_db(move |conn| {
        api_keys::update(&conn, &id, name.as_deref(), rps)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to update key");
        ApiErrorResponse::internal_error("Failed to update key")
    })?;

    if !updated {
        return Err(ApiErrorResponse::not_found("Key not found"));
    }

    info!(key_id = %key_id, "API key updated");
    Ok(responses::ok())
}

/// DELETE /api/v1/admin/keys/:id - Delete a key
pub async fn delete_key_handler(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::api_keys;

    let id = key_id.clone();
    let deleted = state.run_db(move |conn| {
        api_keys::delete(&conn, &id)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to delete key");
        ApiErrorResponse::internal_error("Failed to delete key")
    })?;

    if !deleted {
        return Err(ApiErrorResponse::not_found("Key not found"));
    }

    info!(key_id = %key_id, "API key deleted");
    Ok(responses::ok())
}

// ============================================================================
// Journal Cache Management
// ============================================================================

/// GET /api/v1/admin/cache/journals - List cached journals
pub async fn list_cache_handler(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::journal_cache;

    let page = query.page;
    let limit = query.limit;

    let (journals, total) = state.run_db(move |conn| {
        journal_cache::list(&conn, page, limit)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to list cache");
        ApiErrorResponse::internal_error("Failed to list cache")
    })?;

    let pagination = Pagination::new(page, limit, total);
    Ok(responses::success_paginated(journals, pagination))
}

/// GET /api/v1/admin/cache/stats - Cache statistics
pub async fn cache_stats_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::journal_cache;

    let stats = state.run_db(move |conn| {
        journal_cache::get_stats(&conn)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get cache stats");
        ApiErrorResponse::internal_error("Failed to get cache stats")
    })?;

    Ok(responses::success(stats))
}

/// DELETE /api/v1/admin/cache/journals - Clear all cache
pub async fn clear_cache_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::journal_cache;

    let cleared = state.run_db(move |conn| {
        journal_cache::clear_all(&conn)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to clear cache");
        ApiErrorResponse::internal_error("Failed to clear cache")
    })?;

    info!(cleared = cleared, "Journal cache cleared");

    #[derive(Serialize)]
    struct ClearResult { cleared: usize }
    Ok(responses::success(ClearResult { cleared }))
}

/// DELETE /api/v1/admin/cache/journals/:name - Delete single cache entry
pub async fn delete_cache_entry_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::journal_cache;

    let journal_name = name.clone();
    let deleted = state.run_db(move |conn| {
        journal_cache::delete(&conn, &journal_name)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to delete cache entry");
        ApiErrorResponse::internal_error("Failed to delete cache entry")
    })?;

    if !deleted {
        return Err(ApiErrorResponse::not_found("Cache entry not found"));
    }

    Ok(responses::ok())
}

// ============================================================================
// Analytics & Statistics
// ============================================================================

/// GET /api/v1/admin/stats/overview - Usage overview
pub async fn stats_overview_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::{analytics, journal_cache};

    let result = state.run_db(move |conn| {
        let overview = analytics::get_overview(&conn)?;
        let cache_stats = journal_cache::get_stats(&conn).ok();
        Ok((overview, cache_stats))
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get overview");
        ApiErrorResponse::internal_error("Failed to get overview")
    })?;

    let (overview, cache_stats) = result;

    #[derive(Serialize)]
    struct FullOverview {
        #[serde(flatten)]
        overview: analytics::StatsOverview,
        cache_hit_rate: f64,
    }

    Ok(responses::success(FullOverview {
        cache_hit_rate: cache_stats.map(|c| c.hit_rate).unwrap_or(0.0),
        overview,
    }))
}

/// GET /api/v1/admin/stats/keywords - Top keywords
pub async fn top_keywords_handler(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::analytics;

    let limit = query.limit;
    let keywords = state.run_db(move |conn| {
        analytics::get_top_keywords(&conn, limit)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get keywords");
        ApiErrorResponse::internal_error("Failed to get keywords")
    })?;

    Ok(responses::success(keywords))
}

/// GET /api/v1/admin/stats/journals - Top journals
pub async fn top_journals_handler(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::analytics;

    let limit = query.limit;
    let journals = state.run_db(move |conn| {
        analytics::get_top_journals(&conn, limit)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get journals");
        ApiErrorResponse::internal_error("Failed to get journals")
    })?;

    Ok(responses::success(journals))
}

/// GET /api/v1/admin/stats/daily - Daily statistics
pub async fn daily_stats_handler(
    State(state): State<AppState>,
    Query(query): Query<DaysQuery>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::analytics;

    let days = query.days;
    let daily = state.run_db(move |conn| {
        analytics::get_daily_stats(&conn, days)
    }).await.map_err(|e| {
        error!(error = %e, "Failed to get daily stats");
        ApiErrorResponse::internal_error("Failed to get daily stats")
    })?;

    Ok(responses::success(daily))
}

// ============================================================================
// System Status
// ============================================================================

/// GET /api/v1/admin/system - System status
pub async fn system_status_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    use crate::db::schema::get_db_stats;

    let uptime = state.task_store.uptime();
    let active_tasks = state.task_store.len();

    let db_stats = state.run_db(move |conn| {
        Ok(get_db_stats(&conn).ok())
    }).await.map_err(|e| {
        error!(error = %e, "Database error");
        ApiErrorResponse::internal_error("Database error")
    })?;

    // Get database file size
    let db_size = std::fs::metadata("data/rscholar.db")
        .map(|m| m.len())
        .unwrap_or(0);

    #[derive(Serialize)]
    struct SystemStatus {
        version: String,
        uptime_secs: u64,
        db_size_bytes: u64,
        active_tasks: usize,
        total_api_keys: i64,
        cache_entries: i64,
        total_searches: i64,
    }

    Ok(responses::success(SystemStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime.as_secs(),
        db_size_bytes: db_size,
        active_tasks,
        total_api_keys: db_stats.as_ref().map(|s| s.key_count).unwrap_or(0),
        cache_entries: db_stats.as_ref().map(|s| s.cache_count).unwrap_or(0),
        total_searches: db_stats.as_ref().map(|s| s.search_count).unwrap_or(0),
    }))
}
