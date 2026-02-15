//! Custom error types for Rscholar.
//!
//! This module defines all error types used throughout the application.
//! All functions return `Result<T, GscholarError>` instead of using `unwrap()`.

use thiserror::Error;

/// Main error type for Rscholar operations.
///
/// Uses `thiserror` for ergonomic error handling and automatic `Display` implementation.
#[derive(Debug, Error)]
pub enum GscholarError {
    /// Browser automation error (Playwright)
    #[error("Browser error: {0}")]
    Browser(String),

    /// Network/HTTP request error
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// HTML parsing error
    #[error("Parse error: {0}")]
    Parse(String),

    /// Rate limited by external API
    #[error("Rate limited, retry after {0}s")]
    RateLimited(u64),

    /// External API returned an error
    #[error("API error: {code} - {message}")]
    Api {
        /// Error code from API
        code: i32,
        /// Error message from API
        message: String,
    },

    /// CAPTCHA detected
    #[error("CAPTCHA detected, please refresh cookies")]
    Captcha,

    /// File I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Configuration error
    #[error("Config error: {0}")]
    Config(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// Unauthorized (API key invalid)
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Task not found
    #[error("Task not found: {0}")]
    TaskNotFound(String),
}

/// Result type alias using `GscholarError`
pub type Result<T> = std::result::Result<T, GscholarError>;

/// Extension trait for adding context to Option types
pub trait OptionExt<T> {
    /// Convert Option to Result with a parse error message
    fn ok_or_parse(self, msg: &str) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_parse(self, msg: &str) -> Result<T> {
        self.ok_or_else(|| GscholarError::Parse(msg.to_string()))
    }
}
