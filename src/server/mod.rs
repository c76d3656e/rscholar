//! API Server Module
//!
//! Production HTTP API service with:
//! - Async task queue for long-running pipelines
//! - SQLite persistence with task recovery
//! - Clean separation of concerns
//!
//! Note: Authentication and rate limiting are delegated to
//! external services like Cloudflare WAF.

pub mod admin;
pub mod config;
pub mod handlers;
pub mod pipeline;
pub mod recovery;
pub mod responses;
pub mod routes;
pub mod state;
pub mod task;
pub mod middleware; // Auth logic

