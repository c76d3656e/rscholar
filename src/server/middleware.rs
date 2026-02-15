//! Authentication Middleware
//!
//! Provides API Key validation and Role-Based Access Control (RBAC).

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use crate::server::state::AppState;
use crate::db::api_keys;
use tracing::{debug, warn};

/// Middleware to enforce Admin-only access
///
/// checks:
/// 1. `X-API-Key` header exists
/// 2. Key exists in DB
/// 3. Key has `is_admin = true`
pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Extract Header
    let key_str = match headers.get("x-api-key") {
        Some(v) => match v.to_str() {
            Ok(s) => s.to_string(), // Clone to move into closure
            Err(_) => return Err(StatusCode::UNAUTHORIZED),
        },
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // 2. Database Validation
    let pool = &state.db;
    
    // Get connection
    let conn = pool.get().await.map_err(|e| {
        warn!("DB Pool error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Run blocking validation logic
    let key_str_clone = key_str.clone();
    let validation_result = conn.interact(move |c| {
        api_keys::validate(c, &key_str_clone)
    }).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?; // Join error

    // Check application error
    let key_opt = validation_result.map_err(|e| {
        warn!("Auth DB Error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 3. Authorization Logic
    match key_opt {
        Some(key) => {
            if key.is_admin {
                debug!(id = %key.id, "Admin access granted");
                // Record usage (background) - Optional, preventing block
                // For now we don't block response on usage record
                Ok(next.run(req).await)
            } else {
                warn!(id = %key.id, "Access denied: Not an admin");
                Err(StatusCode::FORBIDDEN)
            }
        }
        None => {
            warn!("Invalid API Key provided");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}
