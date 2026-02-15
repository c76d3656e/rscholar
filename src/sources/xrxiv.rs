//! bioRxiv / medRxiv source client.
//!
//! Shared implementation because both servers use the same API host and
//! almost identical response schema.
//!
//! Endpoint pattern:
//! `https://api.biorxiv.org/details/{server}/{start_date}/{end_date}/{cursor}`

use crate::error::Result;
use crate::sources::rate_limiter;
use crate::sources::{SourcePaper, SourceProvider};
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

const XRXIV_BASE_URL: &str = "https://api.biorxiv.org";
const XRXIV_MAX_HOPS: usize = 40;
const XRXIV_MAX_EMPTY_MATCH_HOPS: usize = 10;
const XRXIV_PAGE_SIZE: usize = 100;
const XRXIV_BATCH_CONCURRENCY: usize = 5;

/// xrxiv server choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum XRxivServer {
    BioRxiv,
    MedRxiv,
}

impl XRxivServer {
    pub fn as_path(&self) -> &'static str {
        match self {
            Self::BioRxiv => "biorxiv",
            Self::MedRxiv => "medrxiv",
        }
    }

    pub fn venue(&self) -> &'static str {
        match self {
            Self::BioRxiv => "bioRxiv",
            Self::MedRxiv => "medRxiv",
        }
    }
}

/// xrxiv query options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XRxivQueryOptions {
    pub max_results: usize,
    pub start_date: String,
    pub end_date: String,
    pub timeout_secs: u64,
    pub request_delay_ms: u64,
    pub max_retries: usize,
}

impl Default for XRxivQueryOptions {
    fn default() -> Self {
        Self {
            max_results: 100,
            start_date: "2020-01-01".to_string(),
            end_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            timeout_secs: 30,
            request_delay_ms: 500,
            max_retries: 3,
        }
    }
}

/// xrxiv normalized record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XRxivResult {
    pub title: String,
    pub authors: String,
    pub year: String,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub pdf_url: String,
    pub abstract_text: String,
}

#[derive(Debug, Deserialize)]
struct XRxivResponse {
    messages: Vec<XRxivMessage>,
    collection: Vec<XRxivPaperRaw>,
}

#[derive(Debug, Deserialize)]
struct XRxivMessage {
    status: Option<String>,
    count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct XRxivPaperRaw {
    doi: Option<String>,
    title: Option<String>,
    authors: Option<String>,
    date: Option<String>,
    version: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
}

/// Provider implementation for bioRxiv / medRxiv.
pub struct XRxivProvider {
    server: XRxivServer,
    client: Client,
}

impl XRxivProvider {
    pub fn new(server: XRxivServer, timeout_secs: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("Rscholar/0.1 (xrxiv source)")
            .build()?;
        Ok(Self { server, client })
    }
}

#[async_trait]
impl SourceProvider<XRxivQueryOptions> for XRxivProvider {
    fn source_name(&self) -> &'static str {
        self.server.as_path()
    }

    async fn search(&self, query: &str, options: &XRxivQueryOptions) -> Result<Vec<SourcePaper>> {
        let papers = search_with_client(&self.client, self.server, query, options).await?;
        Ok(papers
            .into_iter()
            .map(|p| SourcePaper {
                title: p.title,
                authors: p.authors,
                year: p.year,
                venue: p.venue,
                doi: p.doi,
                url: p.url,
                pdf_url: p.pdf_url,
                snippet: String::new(),
                abstract_text: p.abstract_text,
            })
            .collect())
    }
}

/// Search bioRxiv/medRxiv by keyword using date-range cursor pages.
pub async fn search_papers(
    server: XRxivServer,
    query: &str,
    options: &XRxivQueryOptions,
) -> Result<Vec<XRxivResult>> {
    let provider = XRxivProvider::new(server, options.timeout_secs)?;
    search_with_client(&provider.client, server, query, options).await
}

async fn search_with_client(
    client: &Client,
    server: XRxivServer,
    query: &str,
    options: &XRxivQueryOptions,
) -> Result<Vec<XRxivResult>> {
    let max_results = options.max_results.max(1);
    let query_norm = query.to_lowercase();
    let server_path = server.as_path();

    info!(
        source = server_path,
        query = query,
        max_results = max_results,
        start_date = %options.start_date,
        end_date = %options.end_date,
        "Starting xrxiv search"
    );

    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut hops = 0usize;
    let mut empty_match_hops = 0usize;

    while out.len() < max_results && hops < XRXIV_MAX_HOPS {
        let batch_size = XRXIV_BATCH_CONCURRENCY.min(XRXIV_MAX_HOPS - hops);
        let cursors = (0..batch_size)
            .map(|i| cursor + i * XRXIV_PAGE_SIZE)
            .collect::<Vec<_>>();

        let mut pages = stream::iter(cursors.into_iter())
            .map(|page_cursor| async move {
                let endpoint = format!(
                    "{XRXIV_BASE_URL}/details/{}/{}/{}/{}",
                    server_path, options.start_date, options.end_date, page_cursor
                );
                let response = fetch_with_retries(client, &endpoint, options.max_retries).await?;
                Ok::<(usize, XRxivResponse), crate::error::GscholarError>((page_cursor, response))
            })
            .buffer_unordered(XRXIV_BATCH_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        pages.sort_by_key(|(page_cursor, _)| *page_cursor);

        let mut reached_tail = false;
        for (page_cursor, response) in pages {
            let status = response
                .messages
                .first()
                .and_then(|m| m.status.clone())
                .unwrap_or_default();
            if status != "ok" && !status.is_empty() {
                warn!(source = server_path, cursor = page_cursor, status = %status, "xrxiv non-ok status, stopping page scan");
                reached_tail = true;
                break;
            }

            let page_count = response
                .messages
                .first()
                .and_then(|m| m.count)
                .unwrap_or(response.collection.len());

            let before = out.len();
            for raw in response.collection {
                if out.len() >= max_results {
                    break;
                }
                let paper = map_xrxiv_paper(raw, server);
                if !query_norm.trim().is_empty() && !keyword_match(&query_norm, &paper) {
                    continue;
                }
                out.push(paper);
            }

            info!(
                source = server_path,
                cursor = page_cursor,
                page_count = page_count,
                matched = out.len() - before,
                accumulated = out.len(),
                "xrxiv page processed"
            );

            if out.len() == before {
                empty_match_hops += 1;
                if empty_match_hops >= XRXIV_MAX_EMPTY_MATCH_HOPS {
                    warn!(
                        source = server_path,
                        empty_match_hops = empty_match_hops,
                        "xrxiv early stop: too many consecutive pages without keyword matches"
                    );
                    reached_tail = true;
                    break;
                }
            } else {
                empty_match_hops = 0;
            }

            if page_count == 0 {
                reached_tail = true;
                break;
            }
        }

        hops += batch_size;
        cursor += batch_size * XRXIV_PAGE_SIZE;
        if reached_tail || out.len() >= max_results {
            break;
        }
    }

    out.truncate(max_results);
    info!(source = server_path, total = out.len(), "xrxiv search completed");
    Ok(out)
}

async fn fetch_with_retries(client: &Client, endpoint: &str, max_retries: usize) -> Result<XRxivResponse> {
    let mut network_retries = 0usize;
    let mut rate_limit_retries = 0u32;
    let mut server_error_retries = 0u32;

    loop {
        rate_limiter::xrxiv().acquire().await;
        let resp = match client.get(endpoint).send().await {
            Ok(r) => r,
            Err(error) => {
                network_retries += 1;
                if network_retries > max_retries {
                    return Err(error.into());
                }
                warn!(
                    endpoint = endpoint,
                    tries = network_retries,
                    max_retries = max_retries,
                    error = %error,
                    "xrxiv request failed, retrying"
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let status = resp.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            rate_limit_retries += 1;
            let backoff_secs = 1u64 << rate_limit_retries.min(4);
            warn!(
                source = "xrxiv",
                endpoint = endpoint,
                status = 429,
                attempt = rate_limit_retries,
                backoff_secs = backoff_secs,
                "xrxiv rate limited (429), retrying with exponential backoff"
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            continue;
        }

        if status.is_server_error() {
            server_error_retries += 1;
            if server_error_retries > 5 {
                warn!(source = "xrxiv", endpoint = endpoint, status = %status,
                    "xrxiv server error after 5 retries, giving up");
                return Err(crate::error::GscholarError::Api {
                    code: status.as_u16() as i32,
                    message: format!("xrxiv server error: {}", status),
                });
            }
            warn!(
                source = "xrxiv", endpoint = endpoint, status = %status,
                attempt = server_error_retries,
                "xrxiv server error, retrying in 2s"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        let parsed = resp.error_for_status()?.json::<XRxivResponse>().await?;
        return Ok(parsed);
    }
}

fn map_xrxiv_paper(raw: XRxivPaperRaw, server: XRxivServer) -> XRxivResult {
    let doi = raw.doi.unwrap_or_default();
    let version = raw.version.unwrap_or_else(|| "1".to_string());
    let date = raw.date.unwrap_or_default();
    let year = date.get(0..4).unwrap_or_default().to_string();
    let html_url = if doi.is_empty() {
        String::new()
    } else {
        format!("https://www.{}.org/content/{}v{}", server.as_path(), doi, version)
    };
    let pdf_url = if doi.is_empty() {
        String::new()
    } else {
        format!("https://www.{}.org/content/{}v{}.full.pdf", server.as_path(), doi, version)
    };

    XRxivResult {
        title: raw.title.unwrap_or_default(),
        authors: raw.authors.unwrap_or_default(),
        year,
        venue: server.venue().to_string(),
        doi,
        url: html_url,
        pdf_url,
        abstract_text: raw.abstract_text.unwrap_or_default(),
    }
}

fn keyword_match(query_norm: &str, paper: &XRxivResult) -> bool {
    let hay = format!(
        "{} {} {}",
        paper.title.to_lowercase(),
        paper.abstract_text.to_lowercase(),
        paper.authors.to_lowercase()
    );
    query_norm
        .split_whitespace()
        .all(|token| token.is_empty() || hay.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_match() {
        let p = XRxivResult {
            title: "Deep learning for protein design".to_string(),
            abstract_text: "A transformer model for protein sequences".to_string(),
            authors: "Alice Smith".to_string(),
            ..Default::default()
        };
        assert!(keyword_match("deep protein", &p));
        assert!(!keyword_match("deep chemistry", &p));
    }

    #[test]
    fn test_map_xrxiv_paper() {
        let raw = XRxivPaperRaw {
            doi: Some("10.1101/2024.01.15.123456".to_string()),
            title: Some("Sample".to_string()),
            authors: Some("Alice, Bob".to_string()),
            date: Some("2024-01-15".to_string()),
            version: Some("2".to_string()),
            abstract_text: Some("Abstract".to_string()),
        };
        let mapped = map_xrxiv_paper(raw, XRxivServer::BioRxiv);
        assert_eq!(mapped.year, "2024");
        assert!(mapped.pdf_url.contains(".full.pdf"));
        assert_eq!(mapped.venue, "bioRxiv");
    }
}
