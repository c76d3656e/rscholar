//! Ranking Module Tests
//!
//! Tests for EasyScholar ranking client:
//! - Numeric filter validation
//! - String filter validation
//! - RankingMetrics operations

use rscholar::ranking::{LeasePolicy, RankingClient, RankingMetrics, RankingService, RankingServiceOptions};

// ============================================================================
// Filter Tests
// ============================================================================

#[test]
fn test_passes_numeric_filter_above_threshold() {
    assert!(RankingClient::passes_numeric_filter(Some("5.5"), 5.0));
    assert!(RankingClient::passes_numeric_filter(Some("10.0"), 5.0));
    assert!(RankingClient::passes_numeric_filter(Some("5.0"), 5.0)); // Equal
}

#[test]
fn test_passes_numeric_filter_below_threshold() {
    assert!(!RankingClient::passes_numeric_filter(Some("4.9"), 5.0));
    assert!(!RankingClient::passes_numeric_filter(Some("0.1"), 5.0));
}

#[test]
fn test_passes_numeric_filter_invalid_input() {
    assert!(!RankingClient::passes_numeric_filter(None, 5.0));
    assert!(!RankingClient::passes_numeric_filter(Some("invalid"), 5.0));
    assert!(!RankingClient::passes_numeric_filter(Some(""), 5.0));
}

#[test]
fn test_passes_string_filter_exact_match() {
    assert!(RankingClient::passes_string_filter(Some("Q1"), "Q1"));
    assert!(RankingClient::passes_string_filter(Some("Q2"), "Q2"));
}

#[test]
fn test_passes_string_filter_substring_match() {
    assert!(RankingClient::passes_string_filter(Some("Q1/Q2"), "Q1"));
    assert!(RankingClient::passes_string_filter(Some("Top 10%"), "Top"));
}

#[test]
fn test_passes_string_filter_no_match() {
    assert!(!RankingClient::passes_string_filter(Some("Q2"), "Q1"));
    assert!(!RankingClient::passes_string_filter(None, "Q1"));
}

// ============================================================================
// RankingMetrics Tests
// ============================================================================

#[test]
fn test_ranking_metrics_default() {
    let metrics = RankingMetrics::default();
    
    assert!(metrics.sciif.is_none());
    assert!(metrics.jci.is_none());
    assert!(metrics.sci.is_none());
    assert!(metrics.sci_up_top.is_none());
    assert!(metrics.sci_base.is_none());
    assert!(metrics.sci_up.is_none());
}

#[test]
fn test_ranking_metrics_with_values() {
    let metrics = RankingMetrics {
        sciif: Some("10.5".to_string()),
        jci: Some("2.3".to_string()),
        sci: Some("Q1".to_string()),
        sci_up_top: Some("Top 5%".to_string()),
        sci_base: None,
        sci_up: None,
    };
    
    assert_eq!(metrics.sciif.as_deref(), Some("10.5"));
    assert_eq!(metrics.sci.as_deref(), Some("Q1"));
}

#[test]
fn test_get_metric() {
    let metrics = RankingMetrics {
        sciif: Some("15.0".to_string()),
        jci: Some("3.5".to_string()),
        sci: Some("Q1".to_string()),
        sci_up_top: Some("Top 10%".to_string()),
        sci_base: Some("Base Value".to_string()),
        sci_up: Some("Up Value".to_string()),
    };
    
    assert_eq!(RankingClient::get_metric(&metrics, "sciif"), Some("15.0".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "jci"), Some("3.5".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "sci"), Some("Q1".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "sciUpTop"), Some("Top 10%".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "sciBase"), Some("Base Value".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "sciUp"), Some("Up Value".to_string()));
    assert_eq!(RankingClient::get_metric(&metrics, "unknown"), None);
}

#[test]
fn test_ranking_metrics_clone() {
    let original = RankingMetrics {
        sciif: Some("10.0".to_string()),
        jci: None,
        sci: Some("Q1".to_string()),
        sci_up_top: None,
        sci_base: None,
        sci_up: None,
    };
    
    let cloned = original.clone();
    
    assert_eq!(cloned.sciif, original.sciif);
    assert_eq!(cloned.sci, original.sci);
}

#[test]
fn test_ranking_metrics_serialize() {
    let metrics = RankingMetrics {
        sciif: Some("10.0".to_string()),
        jci: None,
        sci: Some("Q1".to_string()),
        sci_up_top: None,
        sci_base: None,
        sci_up: None,
    };
    
    let json = serde_json::to_string(&metrics).expect("serialize");
    assert!(json.contains("\"sciif\":\"10.0\""));
    assert!(json.contains("\"sci\":\"Q1\""));
}

#[test]
fn test_ranking_service_rejects_empty_keyset() {
    let service = RankingService::new(&[], None, RankingServiceOptions::default());
    assert!(service.is_err());
}

#[test]
fn test_ranking_service_default_options() {
    let opts = RankingServiceOptions::default();
    assert_eq!(opts.queue_capacity, 128);
    assert_eq!(opts.lease_policy.min_chunk, 1);
    assert_eq!(opts.lease_policy.max_chunk, 32);
    assert_eq!(opts.max_concurrent_jobs, 16);
    assert_eq!(opts.target_duration_sec, 10);
    assert!((opts.eta_scale - 1.6).abs() < f64::EPSILON);
}

#[test]
fn test_lease_policy_default_min_chunk_is_one() {
    let policy = LeasePolicy::default();
    assert_eq!(policy.min_chunk, 1);
}
