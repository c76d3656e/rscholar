//! Unified API response types.
//!
//! Provides consistent JSON response format for all endpoints.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use tracing::debug;

/// Standard API response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
}

/// Error details
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

/// Pagination info
#[derive(Debug, Clone, Serialize)]
pub struct Pagination {
    pub page: u32,
    pub limit: u32,
    pub total: i64,
    pub total_pages: u32,
}

impl Pagination {
    pub fn new(page: u32, limit: u32, total: i64) -> Self {
        let total_pages = if limit > 0 {
            ((total as u32) + limit - 1) / limit
        } else {
            0
        };
        debug!(page, limit, total, total_pages, "Built pagination response");
        Self {
            page,
            limit,
            total,
            total_pages,
        }
    }
}

/// Success response without pagination
pub fn success<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        pagination: None,
    })
}

/// Success response with pagination
pub fn success_paginated<T: Serialize>(data: T, pagination: Pagination) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        pagination: Some(pagination),
    })
}

/// Error response
pub fn error<T: Serialize>(code: &str, message: &str) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: false,
        data: None,
        error: Some(ApiError {
            code: code.to_string(),
            message: message.to_string(),
        }),
        pagination: None,
    })
}

/// Empty success response
pub fn ok() -> Json<ApiResponse<()>> {
    Json(ApiResponse {
        success: true,
        data: None,
        error: None,
        pagination: None,
    })
}

/// API error with HTTP status code
pub struct ApiErrorResponse {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
}

impl ApiErrorResponse {
    pub fn unauthorized(msg: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED".to_string(),
            message: msg.to_string(),
        }
    }

    pub fn not_found(msg: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: msg.to_string(),
        }
    }

    pub fn bad_request(msg: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BAD_REQUEST".to_string(),
            message: msg.to_string(),
        }
    }

    pub fn internal_error(msg: &str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: msg.to_string(),
        }
    }

    pub fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED".to_string(),
            message: "Too many requests".to_string(),
        }
    }

    pub fn forbidden(msg: &str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: msg.to_string(),
        }
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "success": false,
            "error": {
                "code": self.code,
                "message": self.message
            }
        });
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination() {
        let p = Pagination::new(1, 20, 45);
        assert_eq!(p.total_pages, 3);

        let p2 = Pagination::new(1, 20, 40);
        assert_eq!(p2.total_pages, 2);
    }

    #[test]
    fn test_success_response() {
        let resp = success(vec!["a", "b"]);
        assert!(resp.success);
    }
}
