//! Traffic monitoring module for tracking network usage
//!
//! This module provides a thread-safe global collector to track
//! bytes sent and received during HTTP requests.

use std::sync::atomic::{AtomicU64, Ordering};
use once_cell::sync::Lazy;
use tracing::debug;

/// Global traffic statistics
pub static GLOBAL_TRAFFIC: Lazy<TrafficCollector> = Lazy::new(TrafficCollector::new);

/// Traffic statistics structure
#[derive(Debug, Default, Clone, Copy)]
pub struct TrafficStats {
    /// Total bytes sent (headers + body)
    pub sent: u64,
    /// Total bytes received (headers + body)
    pub received: u64,
}

impl TrafficStats {
    /// Format as "Sent: X KB, Received: Y KB"
    pub fn to_string_kb(&self) -> String {
        format!(
            "Sent: {:.2} KB, Received: {:.2} KB",
            self.sent as f64 / 1024.0,
            self.received as f64 / 1024.0
        )
    }

    /// Format as "Sent: X MB, Received: Y MB"
    pub fn to_string_mb(&self) -> String {
        format!(
            "Sent: {:.2} MB, Received: {:.2} MB",
            self.sent as f64 / 1024.0 / 1024.0,
            self.received as f64 / 1024.0 / 1024.0
        )
    }
}

/// Thread-safe traffic collector
pub struct TrafficCollector {
    sent: AtomicU64,
    received: AtomicU64,
}

impl TrafficCollector {
    /// Create a new collector
    pub fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
        }
    }

    /// Record bytes sent
    pub fn add_sent(&self, bytes: u64) {
        self.sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes received
    pub fn add_received(&self, bytes: u64) {
        self.received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get current snapshot of statistics
    pub fn snapshot(&self) -> TrafficStats {
        TrafficStats {
            sent: self.sent.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
        }
    }

    /// Reset counters to zero
    pub fn reset(&self) {
        self.sent.store(0, Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        debug!("Traffic collector reset");
    }
}

pub trait TrafficTracked {
    fn record_traffic(&self, sent: u64, received: u64);
}
