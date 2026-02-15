//! arXiv API client.
//!
//! Endpoint: `https://export.arxiv.org/api/query`
//! Response: Atom XML feed (`<feed><entry>...</entry></feed>`)
//! Pagination: `start` + `max_results`
//!
//! Notes:
//! - This module performs keyword search and parses essential metadata.
//! - To be polite to arXiv infrastructure, requests are throttled by default.

use crate::error::Result;
use crate::sources::rate_limiter;
use crate::sources::{SourcePaper, SourceProvider};
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

const ARXIV_API_URLS: [&str; 2] = [
    "https://export.arxiv.org/api/query",
    "https://arxiv.org/api/query",
];
const ARXIV_MAX_PER_REQUEST: usize = 2000;

/// arXiv search options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArxivQueryOptions {
    /// Total max results to collect across pages (clamped to 200 in pipeline layer).
    pub max_results: usize,
    /// Per-request page size (`max_results` query param in arXiv API).
    pub page_size: usize,
    /// Sort field (`relevance`, `submittedDate`, `lastUpdatedDate`).
    pub sort_by: String,
    /// Sort order (`ascending`, `descending`).
    pub sort_order: String,
    /// HTTP timeout seconds.
    pub timeout_secs: u64,
    /// Inter-request delay to keep polite RPS.
    pub request_delay_ms: u64,
}

impl Default for ArxivQueryOptions {
    fn default() -> Self {
        Self {
            max_results: 100,
            page_size: 100,
            sort_by: "relevance".to_string(),
            sort_order: "descending".to_string(),
            timeout_secs: 30,
            request_delay_ms: 3000,
        }
    }
}

/// arXiv normalized result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArxivResult {
    pub title: String,
    pub authors: String,
    pub year: String,
    pub doi: String,
    pub url: String,
    pub pdf_url: String,
    pub abstract_text: String,
}

/// arXiv provider implementing the unified source trait.
pub struct ArxivProvider {
    client: Client,
}

impl ArxivProvider {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("Rscholar/0.1 (arxiv source)")
            .build()?;
        Ok(Self { client })
    }
}

#[async_trait]
impl SourceProvider<ArxivQueryOptions> for ArxivProvider {
    fn source_name(&self) -> &'static str {
        "arxiv"
    }

    async fn search(&self, query: &str, options: &ArxivQueryOptions) -> Result<Vec<SourcePaper>> {
        let papers = search_with_client(&self.client, query, options).await?;
        Ok(papers
            .into_iter()
            .map(|p| SourcePaper {
                title: p.title,
                authors: p.authors,
                year: p.year,
                venue: "arXiv".to_string(),
                doi: p.doi,
                url: p.url,
                pdf_url: p.pdf_url,
                snippet: String::new(),
                abstract_text: p.abstract_text,
            })
            .collect())
    }
}

/// Search arXiv and return normalized results.
pub async fn search_papers(query: &str, options: &ArxivQueryOptions) -> Result<Vec<ArxivResult>> {
    let provider = ArxivProvider::new(options.timeout_secs)?;
    search_with_client(&provider.client, query, options).await
}

async fn search_with_client(
    client: &Client,
    query: &str,
    options: &ArxivQueryOptions,
) -> Result<Vec<ArxivResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let max_results = options.max_results.max(1);
    let page_size = options.page_size.clamp(1, ARXIV_MAX_PER_REQUEST);

    info!(
        query = query,
        max_results = max_results,
        page_size = page_size,
        sort_by = %options.sort_by,
        sort_order = %options.sort_order,
        "Starting arXiv search"
    );

    let mut out = Vec::new();
    let mut start = 0usize;

    while out.len() < max_results {
        let remaining = max_results - out.len();
        let batch_size = remaining.min(page_size);

        let params = [
            ("search_query", query.to_string()),
            ("start", start.to_string()),
            ("max_results", batch_size.to_string()),
            ("sortBy", options.sort_by.clone()),
            ("sortOrder", options.sort_order.clone()),
        ];

        let mut resp_text = None;
        let mut last_error = None;

        for endpoint in ARXIV_API_URLS {
            let mut rate_limit_retries = 0u32;
            let mut server_error_retries = 0u32;

            let page_resp = loop {
                rate_limiter::arxiv().acquire().await;
                match client.get(endpoint).query(&params).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                            rate_limit_retries += 1;
                            let backoff_secs = 1u64 << rate_limit_retries.min(4);
                            warn!(
                                source = "arxiv",
                                endpoint = endpoint,
                                status = 429,
                                attempt = rate_limit_retries,
                                backoff_secs = backoff_secs,
                                "arXiv rate limited (429), retrying with exponential backoff"
                            );
                            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                            continue;
                        }
                        if status.is_server_error() {
                            server_error_retries += 1;
                            if server_error_retries > 5 {
                                warn!(source = "arxiv", endpoint = endpoint, status = %status,
                                    "arXiv server error after 5 retries, trying next endpoint");
                                break None;
                            }
                            warn!(
                                source = "arxiv", endpoint = endpoint, status = %status,
                                attempt = server_error_retries,
                                "arXiv server error, retrying in 2s"
                            );
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        match resp.text().await {
                            Ok(text) => break Some(text),
                            Err(err) => {
                                last_error = Some(err.to_string());
                                warn!(
                                    endpoint = endpoint,
                                    start = start,
                                    batch_size = batch_size,
                                    error = %err,
                                    "arXiv response body read failed, trying next endpoint"
                                );
                                break None;
                            }
                        }
                    }
                    Err(err) => {
                        last_error = Some(err.to_string());
                        warn!(
                            endpoint = endpoint,
                            start = start,
                            batch_size = batch_size,
                            error = %err,
                            "arXiv request failed, trying next endpoint"
                        );
                        break None;
                    }
                }
            };

            if let Some(text) = page_resp {
                resp_text = Some(text);
                break;
            }
        }

        let resp_text = resp_text.ok_or_else(|| {
            crate::error::GscholarError::Parse(format!(
                "Failed to fetch arXiv response from all endpoints: {}",
                last_error.unwrap_or_else(|| "unknown error".to_string())
            ))
        })?;

        let page = parse_atom_entries(&resp_text);
        let fetched = page.len();
        info!(
            start = start,
            requested = batch_size,
            fetched = fetched,
            accumulated = out.len() + fetched,
            "arXiv page fetched"
        );
        out.extend(page);

        if fetched == 0 || fetched < batch_size {
            break;
        }

        start += batch_size;
    }

    out.truncate(max_results);
    info!(total = out.len(), "arXiv search completed");
    Ok(out)
}

fn parse_atom_entries(xml: &str) -> Vec<ArxivResult> {
    let entry_re = Regex::new(r"(?s)<entry>(.*?)</entry>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let title_re = Regex::new(r"(?s)<title>(.*?)</title>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let summary_re = Regex::new(r"(?s)<summary>(.*?)</summary>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let published_re = Regex::new(r"(?s)<published>(.*?)</published>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let id_re = Regex::new(r"(?s)<id>(.*?)</id>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let doi_re = Regex::new(r"(?s)<arxiv:doi[^>]*>(.*?)</arxiv:doi>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let author_re =
        Regex::new(r"(?s)<author>\s*<name>(.*?)</name>\s*</author>").unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let pdf_link_re = Regex::new(r#"(?s)<link[^>]*title="pdf"[^>]*href="([^"]+)""#)
        .unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));
    let alt_link_re = Regex::new(r#"(?s)<link[^>]*rel="alternate"[^>]*href="([^"]+)""#)
        .unwrap_or_else(|_| Regex::new("$^").expect("regex fallback"));

    entry_re
        .captures_iter(xml)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .map(|entry| {
            let title = capture_first(&title_re, entry).unwrap_or_default();
            let abstract_text = capture_first(&summary_re, entry).unwrap_or_default();
            let published = capture_first(&published_re, entry).unwrap_or_default();
            let id_url = capture_first(&id_re, entry).unwrap_or_default();
            let doi = capture_first(&doi_re, entry).unwrap_or_default();
            let pdf_url = capture_first(&pdf_link_re, entry).unwrap_or_default();
            let alt_url = capture_first(&alt_link_re, entry).unwrap_or_default();
            let authors = author_re
                .captures_iter(entry)
                .filter_map(|c| c.get(1).map(|m| decode_xml_entities(strip_tags(m.as_str()).trim())))
                .collect::<Vec<_>>()
                .join(", ");

            let year = published.get(0..4).unwrap_or_default().to_string();
            let clean_title = decode_xml_entities(strip_tags(title.as_str()).trim()).replace('\n', " ");
            let clean_abstract = decode_xml_entities(strip_tags(abstract_text.as_str()).trim()).replace('\n', " ");

            ArxivResult {
                title: clean_title,
                authors,
                year,
                doi: decode_xml_entities(doi.trim()),
                url: if !alt_url.is_empty() { alt_url } else { id_url },
                pdf_url,
                abstract_text: clean_abstract,
            }
        })
        .filter(|paper| !paper.title.is_empty())
        .collect()
}

fn capture_first(re: &Regex, text: &str) -> Option<String> {
    re.captures(text)
        .and_then(|cap| cap.get(1).map(|m| decode_xml_entities(strip_tags(m.as_str()).trim())))
}

fn strip_tags(text: &str) -> String {
    let tag_re = Regex::new(r"(?s)<[^>]+>").unwrap_or_else(|_| {
        warn!("Failed to compile XML tag regex, returning raw text");
        Regex::new("$^").expect("regex fallback")
    });
    tag_re.replace_all(text, "").to_string()
}

fn decode_xml_entities(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", " ")
        .replace("&#xA;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atom_entries() {
        let xml = r#"
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/1234.5678v1</id>
    <published>2024-01-10T00:00:00Z</published>
    <title>Test &amp; Title</title>
    <summary>Sample abstract text.</summary>
    <author><name>Alice A</name></author>
    <author><name>Bob B</name></author>
    <arxiv:doi>10.1000/test</arxiv:doi>
    <link rel="alternate" href="http://arxiv.org/abs/1234.5678v1" />
    <link title="pdf" href="http://arxiv.org/pdf/1234.5678v1" />
  </entry>
</feed>
        "#;

        let parsed = parse_atom_entries(xml);
        assert_eq!(parsed.len(), 1);
        let p = &parsed[0];
        assert_eq!(p.year, "2024");
        assert_eq!(p.doi, "10.1000/test");
        assert!(p.authors.contains("Alice A"));
        assert!(p.pdf_url.contains("/pdf/"));
        assert!(p.title.contains("Test & Title"));
    }

    #[test]
    fn test_decode_xml_entities() {
        let s = decode_xml_entities("A &amp; B &lt; C");
        assert_eq!(s, "A & B < C");
    }
}
