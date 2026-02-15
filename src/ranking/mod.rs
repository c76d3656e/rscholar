//! EasyScholar publication rankings API client.
//!
//! Split into focused modules while preserving original pooling behavior.

mod client;
mod pool;
mod response;
mod service;
mod types;

pub use client::RankingClient;
pub use pool::{KeyHealthPolicy, KeyState, RankingClientPool};
pub use service::{LeasePolicy, RankingBatchRequest, RankingBatchResult, RankingService, RankingServiceOptions, SchedulerMode};
pub use types::RankingMetrics;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passes_numeric_filter() {
        assert!(RankingClient::passes_numeric_filter(Some("5.5"), 5.0));
        assert!(!RankingClient::passes_numeric_filter(Some("4.9"), 5.0));
        assert!(!RankingClient::passes_numeric_filter(None, 5.0));
        assert!(!RankingClient::passes_numeric_filter(Some("invalid"), 5.0));
    }

    #[test]
    fn test_passes_string_filter() {
        assert!(RankingClient::passes_string_filter(Some("Q1"), "Q1"));
        assert!(RankingClient::passes_string_filter(Some("Q1/Q2"), "Q1"));
        assert!(!RankingClient::passes_string_filter(Some("Q2"), "Q1"));
        assert!(!RankingClient::passes_string_filter(None, "Q1"));
    }
}
