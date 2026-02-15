//! Semantic Scholar API Client
//!
//! Provides batch lookup of papers by DOI to retrieve abstracts and PDF links.
//!
//! API Details:
//! - Batch endpoint: POST /graph/v1/paper/batch
//! - Max 500 papers per request
//! - 10MB data limit per response
//! - Rate limit: 1 req/s (unauthenticated), higher with API key

use crate::error::{GscholarError, Result};
use crate::sources::rate_limiter;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};
use crate::traffic::GLOBAL_TRAFFIC;

/// Semantic Scholar API base URL
const SS_API_BASE: &str = "https://api.semanticscholar.org/graph/v1";

/// Maximum papers per batch request
const MAX_BATCH_SIZE: usize = 500;

/// Result from Semantic Scholar lookup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticScholarResult {
    pub title: String,
    pub doi: String,
    pub ss_abstract: String,
    pub tldr: String,          // AI-generated one-sentence summary
    pub ss_url: String,
    pub is_oa: bool,
    pub oa_pdf_url: String,
    pub paper_id: String,      // Semantic Scholar paper ID
    pub embedding: String,     // Specter v2 embedding (comma-separated floats)
}

#[derive(Debug, Deserialize)]
struct SSPaper {
    #[serde(rename = "paperId")]
    paper_id: Option<String>,
    title: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    tldr: Option<SSTldr>,
    url: Option<String>,
    #[serde(rename = "isOpenAccess")]
    is_open_access: Option<bool>,
    #[serde(rename = "openAccessPdf")]
    oa_pdf: Option<SSOpenAccessPdf>,
    #[serde(rename = "externalIds")]
    external_ids: Option<SSExternalIds>,
    embedding: Option<SSEmbedding>,
}

#[derive(Debug, Deserialize)]
struct SSTldr {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SSEmbedding {
    vector: Option<Vec<f64>>,
}

#[derive(Debug, Deserialize)]
struct SSOpenAccessPdf {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SSExternalIds {
    #[serde(rename = "DOI")]
    doi: Option<String>,
}

/// Batch lookup papers by DOI using Semantic Scholar API.
///
/// # Arguments
///
/// * `dois` - List of DOI strings (without "DOI:" prefix)
/// * `api_key` - Optional API key for higher rate limits
///
/// # Returns
///
/// List of results for papers found
pub async fn batch_lookup(dois: &[String], api_key: Option<&str>) -> Result<Vec<SemanticScholarResult>> {
    if dois.is_empty() {
        return Ok(Vec::new());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    // Filter out empty DOIs
    let valid_dois: Vec<&String> = dois.iter().filter(|d| !d.is_empty()).collect();
    
    if valid_dois.is_empty() {
        return Ok(Vec::new());
    }

    info!(total = valid_dois.len(), "Starting Semantic Scholar batch lookup");

    // Calculate optimal chunk size
    let total = valid_dois.len();
    let batch_count = if total <= MAX_BATCH_SIZE {
        1
    } else {
        (total + MAX_BATCH_SIZE - 1) / MAX_BATCH_SIZE // ceil division
    };
    let chunk_size = (total + batch_count - 1) / batch_count; // ceil division

    info!(
        batches = batch_count,
        chunk_size = chunk_size,
        "Chunking DOIs for batch requests"
    );

    let mut all_results = Vec::new();

    for (batch_idx, chunk) in valid_dois.chunks(chunk_size).enumerate() {
        info!(
            batch = batch_idx + 1,
            total_batches = batch_count,
            papers = chunk.len(),
            "Processing batch"
        );

        match fetch_batch(&client, chunk, api_key).await {
            Ok(papers) => {
                info!(
                    batch = batch_idx + 1,
                    found = papers.len(),
                    "Batch completed"
                );
                all_results.extend(papers);
            }
            Err(e) => {
                warn!(batch = batch_idx + 1, error = %e, "Batch failed");
                // Continue with other batches
            }
        }

        // Rate limiting handled globally by rate_limiter
        if batch_idx < batch_count - 1 {
            // Small delay to separate debug logs, actual throttling is in rate_limiter
        }
    }

    info!(
        total_found = all_results.len(),
        "Semantic Scholar lookup complete"
    );

    Ok(all_results)
}

/// Fetch a single batch of papers
async fn fetch_batch(
    client: &Client,
    dois: &[&String],
    api_key: Option<&str>,
) -> Result<Vec<SemanticScholarResult>> {
    // Request tldr and embedding.specter_v2 along with other fields
    let url = format!(
        "{}/paper/batch?fields=title,abstract,url,isOpenAccess,openAccessPdf,externalIds,tldr,embedding.specter_v2",
        SS_API_BASE
    );

    // Format DOIs with prefix
    let ids: Vec<String> = dois.iter().map(|d| format!("DOI:{}", d)).collect();

    let body = serde_json::json!({ "ids": ids });

    debug!(url = %url, count = ids.len(), "Sending batch request");

    let mut request = client.post(&url).json(&body);

    // Add API key header if provided
    if let Some(key) = api_key {
        request = request.header("x-api-key", key);
    }

    rate_limiter::semantic_scholar().acquire().await;
    let response = request.send().await?;
    let status = response.status();

    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        warn!(status = status.as_u16(), error = %error_text, "API error");
        return Err(GscholarError::Api {
            code: status.as_u16() as i32,
            message: format!("Semantic Scholar API error: {} - {}", status, error_text),
        });
    }

    let papers: Vec<Option<SSPaper>> = response.json().await.map_err(|e| {
        GscholarError::Parse(format!("Failed to parse Semantic Scholar response: {}", e))
    })?;

    // Convert to our result format
    let results: Vec<SemanticScholarResult> = papers
        .into_iter()
        .filter_map(|p| p)
        .map(|paper| {
            let doi = paper
                .external_ids
                .and_then(|ids| ids.doi)
                .unwrap_or_default();

            // Extract tldr text
            let tldr = paper.tldr.and_then(|t| t.text).unwrap_or_default();

            // Convert embedding vector to comma-separated string
            let embedding = paper
                .embedding
                .and_then(|e| e.vector)
                .map(|v| {
                    v.iter()
                        .map(|f| format!("{:.6}", f))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();

            SemanticScholarResult {
                title: paper.title.unwrap_or_default(),
                doi,
                ss_abstract: paper.abstract_text.unwrap_or_default(),
                tldr,
                ss_url: paper.url.unwrap_or_default(),
                is_oa: paper.is_open_access.unwrap_or(false),
                oa_pdf_url: paper.oa_pdf.and_then(|p| p.url).unwrap_or_default(),
                paper_id: paper.paper_id.unwrap_or_default(),
                embedding,
            }
        })
        .collect();

    Ok(results)
}

// ============================================================================
// Semantic Scholar Search API
// ============================================================================

/// Search result from Semantic Scholar bulk search
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SSSearchPaper {
    pub paper_id: String,
    pub title: String,
    pub authors: String,
    pub venue: String,
    pub year: String,
    pub citation_count: i32,
    pub doi: String,
    pub ss_abstract: String,
    pub url: String,
    pub pdf_url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SSRelevanceSearchResponse {
    offset: usize,
    next: Option<usize>,
    #[allow(dead_code)]
    total: Option<i64>,
    data: Option<Vec<SSSearchPaperRaw>>,
}

#[derive(Debug, Deserialize)]
struct SSSearchPaperRaw {
    #[serde(rename = "paperId")]
    paper_id: Option<String>,
    title: Option<String>,
    authors: Option<Vec<SSAuthor>>,
    venue: Option<String>,
    year: Option<i32>,
    #[serde(rename = "citationCount")]
    citation_count: Option<i32>,
    #[serde(rename = "externalIds")]
    external_ids: Option<SSExternalIds>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    url: Option<String>,
    #[serde(rename = "openAccessPdf")]
    open_access_pdf: Option<SSOpenAccessPdf>,
}

#[derive(Debug, Deserialize)]
struct SSAuthor {
    name: Option<String>,
}

/// Search papers using Semantic Scholar Relevance Search API.
///
/// # Arguments
///
/// * `query` - Search query string
/// * `year_from` - Optional year filter (papers from this year onwards)
/// * `limit` - Maximum number of results to return
/// * `api_key` - Optional API key for higher rate limits
///
/// # Returns
///
/// List of search results filtered by publicationTypes=Review,JournalArticle
pub async fn search_papers(
    query: &str,
    year_from: Option<i32>,
    limit: usize,
    api_key: Option<&str>,
) -> Result<Vec<SSSearchPaper>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let fields = "title,authors,venue,year,citationCount,externalIds,abstract,url,openAccessPdf";
    let publication_types = "Review,JournalArticle";

    info!(
        query = query,
        year_from = ?year_from,
        limit = limit,
        "Starting Semantic Scholar relevance search"
    );

    let traffic_before = GLOBAL_TRAFFIC.snapshot();

    // Build URL - request all results in one call (API supports up to 1000)
    let mut url = format!(
        "{}/paper/search?query={}&fields={}&publicationTypes={}&limit={}",
        SS_API_BASE,
        urlencoding::encode(query),
        fields,
        publication_types,
        limit
    );

    if let Some(year) = year_from {
        url.push_str(&format!("&year={}-", year));
    }

    debug!(url = %url, "Sending search request");

    // Retry loop for rate limiting (429) and server errors (500)
    // Exponential backoff for 429: 1s, 2s, 4s, 8s, 16s
    // Max 5 retries for server errors to prevent infinite loops
    let mut rate_limit_retries = 0u32;
    let mut server_error_retries = 0;
    let response = loop {
        let mut request = client.get(&url);
        if let Some(key) = api_key {
            request = request.header("x-api-key", key);
        }

        rate_limiter::semantic_scholar().acquire().await;
        let resp = request.send().await?;
        let status = resp.status();
        if status == 429 {
            rate_limit_retries += 1;
            let backoff_secs = 1u64 << rate_limit_retries.min(4); // 1, 2, 4, 8, 16 seconds
            warn!("SS Rate limit (429), waiting {}s before retry (attempt {})...", backoff_secs, rate_limit_retries);
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            continue;
        }
        if status == 500 || status == 502 || status == 503 {
            server_error_retries += 1;
            if server_error_retries > 5 {
                warn!("SS Server error ({}) after 5 retries, giving up", status);
                break resp;
            }
            warn!("SS Server error ({}), waiting 2s before retry ({}/5)...", status, server_error_retries);
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        break resp;
    };

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        warn!(status = status.as_u16(), error = %error_text, "SS Search API error");
        return Err(GscholarError::Api {
            code: status.as_u16() as i32,
            message: format!("Semantic Scholar Search error: {} - {}", status, error_text),
        });
    }

    // Measure traffic
    let text = response.text().await?;
    
    // Estimate traffic (URL + headers overhead vs body)
    GLOBAL_TRAFFIC.add_sent(url.len() as u64 + 500); // 500 bytes est for headers
    GLOBAL_TRAFFIC.add_received(text.len() as u64);

    let resp: SSRelevanceSearchResponse = serde_json::from_str(&text).map_err(|e| {
        GscholarError::Parse(format!("Failed to parse SS search response: {}", e))
    })?;

    let mut results = Vec::new();
    if let Some(data) = resp.data {
        for paper in data {
            let authors = paper
                .authors
                .map(|a| {
                    a.into_iter()
                        .filter_map(|au| au.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();

            let doi = paper
                .external_ids
                .and_then(|ids| ids.doi)
                .unwrap_or_default();

            let result = SSSearchPaper {
                paper_id: paper.paper_id.unwrap_or_default(),
                title: paper.title.unwrap_or_default(),
                authors,
                venue: paper.venue.unwrap_or_default(),
                year: paper.year.map(|y| y.to_string()).unwrap_or_default(),
                citation_count: paper.citation_count.unwrap_or(0),
                doi,
                ss_abstract: paper.abstract_text.unwrap_or_default(),
                url: paper.url.unwrap_or_default(),
                pdf_url: paper.open_access_pdf.and_then(|p| p.url).unwrap_or_default(),
            };

            results.push(result);
        }
    }

    let traffic_after = GLOBAL_TRAFFIC.snapshot();
    let sent_kb = (traffic_after.sent - traffic_before.sent) as f64 / 1024.0;
    let recv_kb = (traffic_after.received - traffic_before.received) as f64 / 1024.0;

    info!(
        total = results.len(),
        traffic_sent_kb = sent_kb,
        traffic_recv_kb = recv_kb,
        "Semantic Scholar search complete"
    );
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_chunking() {
        // Test that chunking logic is correct
        let total = 1200;
        let batch_count = (total + MAX_BATCH_SIZE - 1) / MAX_BATCH_SIZE;
        assert_eq!(batch_count, 3); // ceil(1200/500) = 3
        
        let chunk_size = (total + batch_count - 1) / batch_count;
        assert_eq!(chunk_size, 400); // ceil(1200/3) = 400
    }
}
