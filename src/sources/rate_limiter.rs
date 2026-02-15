//! Global per-source rate limiter.
//!
//! Provides process-wide (server-wide) request rate limiting for external APIs.
//! Each API source gets a singleton `RateLimiter` that enforces a maximum
//! requests-per-second (RPS) regardless of how many concurrent tasks are running.
//!
//! Uses a token-bucket approach backed by `tokio::sync::Semaphore` and a
//! spawned replenish task.

use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::Semaphore;
use tracing::debug;

/// A token-bucket rate limiter that enforces a global max RPS.
///
/// Callers must call [`acquire`](RateLimiter::acquire) before each HTTP request.
/// The call will block asynchronously until a token becomes available.
pub struct RateLimiter {
    name: &'static str,
    semaphore: Arc<Semaphore>,
    /// Minimum interval between requests in microseconds.
    interval_us: u64,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// * `name`    – human-readable label (for logging)
    /// * `max_rps` – maximum requests per second (process-wide)
    /// * `burst`   – token bucket burst size (how many requests can fire
    ///               simultaneously before throttling kicks in)
    pub fn new(name: &'static str, max_rps: f64, burst: usize) -> Self {
        let interval_us = (1_000_000.0 / max_rps) as u64;
        Self {
            name,
            semaphore: Arc::new(Semaphore::new(burst)),
            interval_us,
        }
    }

    /// Wait until a request is permitted, then return.
    ///
    /// This method is cancel-safe.
    pub async fn acquire(&self) {
        // Acquire a permit (blocks if burst budget is exhausted).
        let permit = self
            .semaphore
            .acquire()
            .await
            .expect("rate limiter semaphore closed unexpectedly");

        // We intentionally forget the permit and add it back after the delay.
        // This ensures the token is only replenished after the inter-request
        // interval has elapsed, enforcing a steady-state RPS cap.
        permit.forget();

        let interval_us = self.interval_us;
        let name = self.name;
        let sem = Arc::clone(&self.semaphore);

        // Spawn a task that returns the token after the interval.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_micros(interval_us)).await;
            sem.add_permits(1);
            debug!(source = name, "rate limiter token replenished");
        });
    }
}

// ---------------------------------------------------------------------------
// Per-source global singletons
// ---------------------------------------------------------------------------

static PUBMED_KEYED: OnceLock<RateLimiter> = OnceLock::new();
static PUBMED_UNKEYED: OnceLock<RateLimiter> = OnceLock::new();
static ARXIV: OnceLock<RateLimiter> = OnceLock::new();
static SEMANTIC_SCHOLAR: OnceLock<RateLimiter> = OnceLock::new();
static OPENALEX: OnceLock<RateLimiter> = OnceLock::new();
static XRXIV: OnceLock<RateLimiter> = OnceLock::new();
static CROSSREF: OnceLock<RateLimiter> = OnceLock::new();

/// PubMed: 10 RPS with API key, 3 RPS without.
/// We use conservative limits (8 / 2) to leave headroom.
pub fn pubmed(has_api_key: bool) -> &'static RateLimiter {
    if has_api_key {
        PUBMED_KEYED.get_or_init(|| RateLimiter::new("pubmed", 8.0, 2))
    } else {
        PUBMED_UNKEYED.get_or_init(|| RateLimiter::new("pubmed", 2.0, 1))
    }
}

/// arXiv: recommended 1 RPS.
pub fn arxiv() -> &'static RateLimiter {
    ARXIV.get_or_init(|| RateLimiter::new("arxiv", 1.0, 1))
}

/// Semantic Scholar: 1 RPS without key.
pub fn semantic_scholar() -> &'static RateLimiter {
    SEMANTIC_SCHOLAR.get_or_init(|| RateLimiter::new("semanticscholar", 1.0, 1))
}

/// OpenAlex polite pool: 10 RPS.
pub fn openalex() -> &'static RateLimiter {
    OPENALEX.get_or_init(|| RateLimiter::new("openalex", 9.0, 2))
}

/// bioRxiv / medRxiv: 10 RPS.
pub fn xrxiv() -> &'static RateLimiter {
    XRXIV.get_or_init(|| RateLimiter::new("xrxiv", 9.0, 3))
}

/// Crossref polite pool: 10 RPS.
pub fn crossref() -> &'static RateLimiter {
    CROSSREF.get_or_init(|| RateLimiter::new("crossref", 9.0, 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_rate_limiter_timing() {
        // 10 RPS limiter with burst=1
        let limiter = RateLimiter::new("test", 10.0, 1);

        let start = Instant::now();
        // First acquire should be instant (burst = 1).
        limiter.acquire().await;

        // Next 4 acquires should be throttled.
        for _ in 0..4 {
            limiter.acquire().await;
        }
        let elapsed = start.elapsed();

        // 5 total acquires at 10 RPS = 4 intervals of 100ms = ~400ms minimum
        // Allow some tolerance
        assert!(
            elapsed.as_millis() >= 350,
            "Expected >= 350ms, got {}ms",
            elapsed.as_millis()
        );
        assert!(
            elapsed.as_millis() <= 800,
            "Expected <= 800ms, got {}ms (too slow)",
            elapsed.as_millis()
        );

        println!("5 acquires at 10 RPS took {}ms", elapsed.as_millis());
    }

    #[tokio::test]
    async fn test_rate_limiter_burst() {
        // 5 RPS with burst=3
        let limiter = RateLimiter::new("test_burst", 5.0, 3);

        let start = Instant::now();
        // First 3 should be instant (burst).
        for _ in 0..3 {
            limiter.acquire().await;
        }
        let burst_elapsed = start.elapsed();

        assert!(
            burst_elapsed.as_millis() < 50,
            "Burst should be near-instant, got {}ms",
            burst_elapsed.as_millis()
        );

        // Next acquire should wait ~200ms (1/5 RPS)
        limiter.acquire().await;
        let total = start.elapsed();
        assert!(
            total.as_millis() >= 150,
            "Post-burst acquire should wait, total={}ms",
            total.as_millis()
        );

        println!(
            "Burst: {}ms, 4th acquire at: {}ms",
            burst_elapsed.as_millis(),
            total.as_millis()
        );
    }
}
