//! Crossref API client for metadata enrichment.
//!
//! This module provides concurrent lookup of article metadata via the Crossref API,
//! including DOI, journal name, authors, publication date, and abstract.

use crate::error::{GscholarError, Result};
use crate::sources::rate_limiter;
use futures::future::join_all;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// Crossref API base URL
const CROSSREF_API_URL: &str = "https://api.crossref.org/works";

/// Polite pool email for Crossref API
const MAILTO: &str = "gscholar-rust@example.com";

/// Enriched metadata from Crossref
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossrefMetadata {
    /// DOI
    pub doi: String,
    /// Journal name
    pub journal: String,
    /// Authors (comma-separated)
    pub authors: String,
    /// Publication date (YYYY-MM-DD or partial)
    pub date: String,
    /// Article abstract (HTML tags stripped)
    pub abstract_text: String,
    /// Title from Crossref (for verification)
    pub crossref_title: String,
}

/// Crossref API client with rate limiting and concurrency control
pub struct CrossrefClient {
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
    max_retries: u32,
}

impl CrossrefClient {
    /// Create a new CrossrefClient
    ///
    /// # Arguments
    ///
    /// * `max_workers` - Maximum concurrent requests (default: 3)
    pub fn new(max_workers: usize) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(format!("gscholar-rust/1.0 (mailto:{})", MAILTO))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| GscholarError::Config(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            semaphore: Arc::new(Semaphore::new(max_workers)),
            max_retries: 3,
        })
    }

    /// Lookup article metadata by title
    ///
    /// Uses exponential backoff for rate limiting
    pub async fn lookup_by_title(&self, title: &str) -> Option<CrossrefMetadata> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }

        let _permit = self.semaphore.acquire().await.ok()?;

        let mut backoff = Duration::from_millis(500);

        for attempt in 0..self.max_retries {
            match self.do_lookup(title).await {
                Ok(Some(metadata)) => return Some(metadata),
                Ok(None) => return None,
                Err(GscholarError::RateLimited(secs)) => {
                    let wait = Duration::from_secs(secs).max(backoff);
                    warn!(
                        title = &title[..title.len().min(30)],
                        attempt = attempt + 1,
                        wait_secs = wait.as_secs(),
                        "Rate limited, waiting"
                    );
                    tokio::time::sleep(wait).await;
                    backoff *= 2;
                }
                Err(e) => {
                    debug!(
                        title = &title[..title.len().min(30)],
                        attempt = attempt + 1,
                        error = %e,
                        "Lookup failed"
                    );
                    if attempt < self.max_retries - 1 {
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                    }
                }
            }
        }

        None
    }

    /// Internal lookup implementation
    async fn do_lookup(&self, title: &str) -> Result<Option<CrossrefMetadata>> {
        rate_limiter::crossref().acquire().await;
        let response = self
            .client
            .get(CROSSREF_API_URL)
            .query(&[
                ("query.title", title),
                ("rows", "1"),
                ("select", "DOI,title,author,container-title,published,abstract"),
                ("mailto", MAILTO),
            ])
            .send()
            .await?;

        // Check rate limit headers
        if let Some(limit) = response.headers().get("X-Rate-Limit-Limit") {
            debug!(limit = ?limit, "Rate limit");
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(GscholarError::RateLimited(5));
        }

        if !response.status().is_success() {
            return Err(GscholarError::Api {
                code: response.status().as_u16() as i32,
                message: format!("Crossref API error: {}", response.status()),
            });
        }

        let data: CrossrefResponse = response.json().await?;

        if let Some(item) = data.message.items.into_iter().next() {
            Ok(Some(parse_crossref_item(item)))
        } else {
            Ok(None)
        }
    }

    /// Lookup multiple titles concurrently
    ///
    /// Returns a vector with the same length as input, with None for failed lookups
    pub async fn lookup_batch(&self, titles: &[String]) -> Vec<Option<CrossrefMetadata>> {
        info!(count = titles.len(), "Starting batch Crossref lookup");

        let futures: Vec<_> = titles
            .iter()
            .map(|title| self.lookup_by_title(title))
            .collect();

        let results = join_all(futures).await;

        let matched = results.iter().filter(|r| r.is_some()).count();
        info!(
            total = titles.len(),
            matched = matched,
            "Batch lookup complete"
        );

        results
    }
}

impl Default for CrossrefClient {
    fn default() -> Self {
        Self::new(3).unwrap_or_else(|_| Self {
            client: reqwest::Client::new(),
            semaphore: Arc::new(Semaphore::new(3)),
            max_retries: 3,
        })
    }
}

// === Crossref API Response Types ===

#[derive(Debug, Deserialize)]
struct CrossrefResponse {
    message: CrossrefMessage,
}

#[derive(Debug, Deserialize)]
struct CrossrefMessage {
    #[serde(default)]
    items: Vec<CrossrefItem>,
}

#[derive(Debug, Deserialize)]
struct CrossrefItem {
    #[serde(rename = "DOI", default)]
    doi: String,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<CrossrefAuthor>,
    #[serde(rename = "container-title", default)]
    container_title: Vec<String>,
    #[serde(default)]
    published: Option<CrossrefPublished>,
    #[serde(rename = "abstract", default)]
    abstract_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    #[serde(default)]
    given: String,
    #[serde(default)]
    family: String,
}

#[derive(Debug, Deserialize)]
struct CrossrefPublished {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<i32>>,
}

/// Parse Crossref API item into our metadata struct
fn parse_crossref_item(item: CrossrefItem) -> CrossrefMetadata {
    // Authors
    let authors = item
        .author
        .iter()
        .map(|a| format!("{} {}", a.given, a.family).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");

    // Date
    let date = item
        .published
        .and_then(|p| p.date_parts.into_iter().next())
        .map(|parts| {
            parts
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("-")
        })
        .unwrap_or_default();

    // Journal
    let journal = item.container_title.into_iter().next().unwrap_or_default();

    // Abstract (strip HTML tags)
    let abstract_text = item
        .abstract_text
        .map(|s| strip_html_tags(&s))
        .unwrap_or_default();

    // Title
    let crossref_title = item.title.into_iter().next().unwrap_or_default();

    CrossrefMetadata {
        doi: item.doi,
        journal,
        authors,
        date,
        abstract_text,
        crossref_title,
    }
}

/// Strip HTML tags from text
fn strip_html_tags(text: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap_or_else(|_| Regex::new(r"").expect("Empty regex"));
    re.replace_all(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(strip_html_tags("No tags"), "No tags");
        assert_eq!(
            strip_html_tags("<b>Bold</b> and <i>italic</i>"),
            "Bold and italic"
        );
    }

    #[test]
    fn test_parse_crossref_item() {
        let item = CrossrefItem {
            doi: "10.1234/test".to_string(),
            title: vec!["Test Title".to_string()],
            author: vec![CrossrefAuthor {
                given: "John".to_string(),
                family: "Doe".to_string(),
            }],
            container_title: vec!["Nature".to_string()],
            published: Some(CrossrefPublished {
                date_parts: vec![vec![2023, 6, 15]],
            }),
            abstract_text: Some("<p>This is abstract</p>".to_string()),
        };

        let metadata = parse_crossref_item(item);
        assert_eq!(metadata.doi, "10.1234/test");
        assert_eq!(metadata.authors, "John Doe");
        assert_eq!(metadata.date, "2023-6-15");
        assert_eq!(metadata.abstract_text, "This is abstract");
    }
}
