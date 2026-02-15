use crate::error::{GscholarError, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::response::{extract_metrics, EasyScholarResponse};
use super::types::RankingMetrics;

/// EasyScholar API base URL
const EASYSCHOLAR_API_URL: &str = "https://www.easyscholar.cc/open/getPublicationRank";
/// Requests per second per API key
const EASYSCHOLAR_RPS_PER_KEY: u64 = 2;
/// Minimum interval between requests, calculated from RPS
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(1000 / EASYSCHOLAR_RPS_PER_KEY);

/// EasyScholar API client with caching and rate limiting
pub struct RankingClient {
    secret_key: String,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, Option<RankingMetrics>>>,
    last_request: Mutex<Option<Instant>>,
}

impl RankingClient {
    pub fn new(secret_key: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| GscholarError::Config(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            secret_key,
            client,
            cache: Mutex::new(HashMap::new()),
            last_request: Mutex::new(None),
        })
    }

    pub async fn get_rank(&self, venue_name: &str) -> Option<RankingMetrics> {
        self.get_rank_with_status(venue_name).await.0
    }

    /// Query one venue and return both data and request health state.
    ///
    /// `request_ok = true` means HTTP exchange and parsing succeeded
    /// (even if venue has no ranking data).
    /// `request_ok = false` means transport/API failure and should count
    /// toward key health degradation.
    pub async fn get_rank_with_status(&self, venue_name: &str) -> (Option<RankingMetrics>, bool) {
        let venue_name = venue_name.trim();
        if venue_name.is_empty() {
            return (None, true);
        }

        {
            let cache = match self.cache.lock() {
                Ok(v) => v,
                Err(_) => return (None, false),
            };
            if let Some(cached) = cache.get(venue_name) {
                debug!(venue = venue_name, "In-memory cache hit");
                return (cached.clone(), true);
            }
        }

        self.wait_for_rate_limit().await;
        let (result, request_ok) = match self.do_request(venue_name).await {
            Ok(value) => (value, true),
            Err(error) => {
                warn!(venue = venue_name, error = %error, "EasyScholar request failed");
                (None, false)
            }
        };

        if request_ok {
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(venue_name.to_string(), result.clone());
            }
        }

        (result, request_ok)
    }

    async fn wait_for_rate_limit(&self) {
        let should_wait = {
            let last = self.last_request.lock().ok();
            last.and_then(|l| *l).map(|t| t.elapsed() < MIN_REQUEST_INTERVAL)
        };

        if should_wait == Some(true) {
            tokio::time::sleep(MIN_REQUEST_INTERVAL).await;
        }

        if let Ok(mut last) = self.last_request.lock() {
            *last = Some(Instant::now());
        }
    }

    async fn do_request(&self, venue_name: &str) -> Result<Option<RankingMetrics>> {
        debug!(venue = venue_name, "Querying EasyScholar");

        let response = self
            .client
            .get(EASYSCHOLAR_API_URL)
            .query(&[
                ("secretKey", self.secret_key.as_str()),
                ("publicationName", venue_name),
            ])
            .send()
            .await
            .map_err(|e| GscholarError::Api {
                code: 503,
                message: format!("EasyScholar request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let code = response.status().as_u16() as i32;
            warn!(
                venue = venue_name,
                status = code,
                "EasyScholar API error"
            );
            return Err(GscholarError::Api {
                code,
                message: "EasyScholar API returned non-success status".to_string(),
            });
        }

        let data: EasyScholarResponse = match response.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!(venue = venue_name, error = %e, "Failed to parse response");
                return Err(GscholarError::Parse(format!(
                    "Failed to parse EasyScholar response: {}",
                    e
                )));
            }
        };

        if data.code != 200 {
            warn!(
                venue = venue_name,
                code = data.code,
                msg = data.msg.as_deref().unwrap_or("Unknown"),
                "EasyScholar API returned error"
            );
            return Err(GscholarError::Api {
                code: data.code,
                message: data.msg.unwrap_or_else(|| "EasyScholar business error".to_string()),
            });
        }

        let result = data.data.map(|d| extract_metrics(&d)).unwrap_or_default();
        let has_data = result.sciif.is_some() || result.jci.is_some() || result.sci.is_some();
        if has_data {
            info!(venue = venue_name, "Found ranking data");
        } else {
            debug!(venue = venue_name, "No ranking data found (will cache empty)");
        }
        Ok(Some(result))
    }

    pub fn get_metric(metrics: &RankingMetrics, key: &str) -> Option<String> {
        match key {
            "sciif" => metrics.sciif.clone(),
            "jci" => metrics.jci.clone(),
            "sci" => metrics.sci.clone(),
            "sciUpTop" => metrics.sci_up_top.clone(),
            "sciBase" => metrics.sci_base.clone(),
            "sciUp" => metrics.sci_up.clone(),
            _ => None,
        }
    }

    pub fn passes_numeric_filter(value: Option<&str>, threshold: f64) -> bool {
        value
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v >= threshold)
            .unwrap_or(false)
    }

    pub fn passes_string_filter(value: Option<&str>, pattern: &str) -> bool {
        value.map(|v| v.contains(pattern)).unwrap_or(false)
    }

    pub async fn get_rank_cached(
        &self,
        venue_name: &str,
        conn: Option<&rusqlite::Connection>,
    ) -> Option<RankingMetrics> {
        let venue_name = venue_name.trim();
        if venue_name.is_empty() {
            return None;
        }

        if let Some(c) = conn {
            if let Ok(Some(cached)) = crate::db::journal_cache::get(c, venue_name) {
                debug!(venue = venue_name, "SQLite DB cache hit");
                return Some(RankingMetrics {
                    sciif: cached.sciif,
                    jci: cached.jci,
                    sci: cached.sci,
                    sci_up_top: cached.sci_up_top,
                    sci_base: cached.sci_base,
                    sci_up: cached.sci_up,
                });
            }
        }

        let result = self.get_rank(venue_name).await;
        if let Some(ref metrics) = result {
            if let Some(c) = conn {
                let cache_entry = crate::db::journal_cache::JournalRanking {
                    name: venue_name.to_string(),
                    sciif: metrics.sciif.clone(),
                    jci: metrics.jci.clone(),
                    sci: metrics.sci.clone(),
                    sci_up_top: metrics.sci_up_top.clone(),
                    sci_base: metrics.sci_base.clone(),
                    sci_up: metrics.sci_up.clone(),
                    fetched_at: chrono::Utc::now().timestamp(),
                };
                if let Err(e) = crate::db::journal_cache::upsert(c, &cache_entry) {
                    warn!(error = %e, "Failed to cache ranking");
                }
            }
        }
        result
    }
}
