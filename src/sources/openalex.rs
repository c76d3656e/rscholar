//! OpenAlex API Client
//!
//! Provides search functionality using the OpenAlex API as an alternative to Google Scholar.
//! OpenAlex returns DOI directly, so Crossref enrichment can be skipped.
//!
//! API Best Practices (per OpenAlex docs):
//! - Use `mailto:email` parameter for polite pool (10 req/s vs 1 req/s)
//! - Use `per-page=200` for maximum results per page
//! - Implement exponential backoff for retries

use crate::error::{GscholarError, Result};
use crate::sources::rate_limiter;
use chrono::Datelike;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use std::time::Duration;

/// OpenAlex API base URL
const OPENALEX_API_BASE: &str = "https://api.openalex.org";

/// Maximum results per page (OpenAlex limit)
const MAX_PER_PAGE: usize = 200;

/// Email for polite pool access
const POLITE_EMAIL: &str = "c76d@c.com";

/// Result from OpenAlex search (expanded fields)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAlexResult {
    pub title: String,
    pub author: String,
    pub year: String,
    pub publication_date: String,  // ISO 8601 date
    pub venue: String,             // Journal/source name
    pub source_type: String,       // journal, repository, etc.
    pub doi: String,
    pub article_url: String,       // Landing page URL
    pub pdf_url: String,           // Direct PDF URL (if available)
    pub citations: String,         // Cited by count
    pub snippet: String,           // Abstract excerpt
    pub openalex_id: String,       // OpenAlex work ID
    // Open Access info
    pub is_oa: bool,
    pub oa_status: String,         // gold, green, hybrid, bronze, closed
    pub oa_url: String,            // Best OA URL
    // Additional metadata
    pub language: String,          // ISO 639-1 language code
    pub work_type: String,         // article, book, etc.
    pub keywords: String,          // Comma-separated keywords
    pub primary_topic: String,     // Primary research topic
    // Reference info (full lists as comma-separated OpenAlex IDs)
    pub referenced_works: String,  // Works this paper cites
    pub related_works: String,     // Algorithmically related works
    pub referenced_works_count: i64,
    pub related_works_count: i64,
    // All locations count
    pub locations_count: i64,
}

/// Query options for OpenAlex search
#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Page numbers to fetch (1-indexed)
    pub pages: Vec<i32>,
    /// Year low filter (results from this year onwards)
    pub ylo: Option<i32>,
    /// Year high filter (results up to this year)
    pub yhi: Option<i32>,
    /// Whether to return all results or just first per page
    pub all_results: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            pages: vec![1],
            ylo: None,
            yhi: None,
            all_results: true,
        }
    }
}

/// OpenAlex API response structures
#[derive(Debug, Deserialize)]
struct OpenAlexResponse {
    #[allow(dead_code)]
    meta: OpenAlexMeta,
    results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexMeta {
    #[allow(dead_code)]
    count: i64,
    #[allow(dead_code)]
    per_page: i32,
    #[allow(dead_code)]
    page: i32,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    id: Option<String>,
    title: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i32>,
    publication_date: Option<String>,
    doi: Option<String>,
    cited_by_count: Option<i64>,
    #[serde(rename = "abstract_inverted_index")]
    abstract_index: Option<serde_json::Value>,
    authorships: Option<Vec<OpenAlexAuthorship>>,
    primary_location: Option<OpenAlexLocation>,
    best_oa_location: Option<OpenAlexLocation>,
    open_access: Option<OpenAlexOpenAccess>,
    language: Option<String>,
    #[serde(rename = "type")]
    work_type: Option<String>,
    keywords: Option<Vec<OpenAlexKeyword>>,
    primary_topic: Option<OpenAlexTopic>,
    referenced_works: Option<Vec<String>>,
    referenced_works_count: Option<i64>,
    related_works: Option<Vec<String>>,
    locations_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAlexLocation {
    source: Option<OpenAlexSource>,
    landing_page_url: Option<String>,
    pdf_url: Option<String>,
    is_oa: Option<bool>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAlexSource {
    display_name: Option<String>,
    issn_l: Option<String>,
    #[serde(rename = "type")]
    source_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAlexOpenAccess {
    is_oa: Option<bool>,
    oa_status: Option<String>,
    oa_url: Option<String>,
    any_repository_has_fulltext: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexKeyword {
    display_name: Option<String>,
    #[allow(dead_code)]
    score: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexTopic {
    display_name: Option<String>,
    #[allow(dead_code)]
    id: Option<String>,
}

/// Query OpenAlex for academic works.
///
/// # Arguments
///
/// * `search_query` - The search keywords
/// * `options` - Query options (pages, year filters)
///
/// # Returns
///
/// List of search results
pub async fn query(search_query: &str, options: &QueryOptions) -> Result<Vec<OpenAlexResult>> {
    use futures::stream::{self, StreamExt};
    
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("rscholar/1.0 (mailto:c76d@c.com)")
        .build()?;

    info!(
        query = search_query,
        pages = ?options.pages,
        ylo = ?options.ylo,
        "Starting OpenAlex query"
    );

    // Enforce hard limit of 200 results (1 page)
    // Even if multiple pages requested, we only take the first one
    let target_page = options.pages.first().copied().unwrap_or(1);
    let pages_to_fetch = vec![target_page];

    // Build URLs for the single target page
    let urls: Vec<(i32, String)> = pages_to_fetch.iter()
        .filter_map(|page| {
            build_search_url(search_query, *page, options)
                .ok()
                .map(|url| (*page, url))
        })
        .collect();

    // Concurrent fetching with 5 parallel requests (polite pool allows 10/s)
    let concurrent_limit = 5;
    
    let results: Vec<(i32, std::result::Result<Vec<OpenAlexResult>, GscholarError>)> = stream::iter(urls)
        .map(|(page, url)| {
            let client = client.clone();
            async move {
                debug!(url = %url, page = page, "Fetching OpenAlex page");
                match fetch_page(&client, &url).await {
                    Ok(response) => {
                        let works = parse_response(&response);
                        (page, works)
                    }
                    Err(e) => {
                        warn!(page = page, error = %e, "Failed to fetch page");
                        (page, Ok(Vec::new()))
                    }
                }
            }
        })
        .buffer_unordered(concurrent_limit)
        .collect()
        .await;

    // Combine results in page order
    let mut all_results = Vec::new();
    let mut sorted_results: Vec<_> = results.into_iter().collect();
    sorted_results.sort_by_key(|(page, _)| *page);
    
    for (page, result) in sorted_results {
        match result {
            Ok(works) => {
                let count = works.len();
                info!(page = page, count = count, "Parsed OpenAlex results");
                if options.all_results {
                    all_results.extend(works);
                } else if let Some(first) = works.into_iter().next() {
                    all_results.push(first);
                }
            }
            Err(e) => {
                warn!(page = page, error = %e, "Parse error");
            }
        }
    }

    info!(total = all_results.len(), "OpenAlex query complete");
    Ok(all_results)
}

/// Build OpenAlex API search URL
fn build_search_url(query: &str, page: i32, options: &QueryOptions) -> Result<String> {
    // Build filter params - use title_and_abstract.search for precise matching
    // This matches OpenAlex web interface behavior and gives much better results
    let mut filters = Vec::new();
    
    // Primary search filter: search in title and abstract only (not full text)
    // Only encode the query itself, not the filter structure
    // Space should be encoded as + for OpenAlex compatibility
    let encoded_query = query.replace(' ', "+");
    filters.push(format!("title_and_abstract.search:{}", encoded_query));
    
    // Add year filters
    if let Some(ylo) = options.ylo {
        let current_year = chrono::Utc::now().year();
        filters.push(format!("publication_year:{}-{}", ylo, current_year));
    } else if let Some(yhi) = options.yhi {
        // Only yhi provided
        filters.push(format!("publication_year:<={}", yhi));
    }

    // Filter for journal articles only (type:article)
    filters.push("type:article".to_string());

    // Don't URL-encode the entire filter - OpenAlex expects specific format
    // filter=title_and_abstract.search:measure+while+drilling,publication_year:2021-2026,type:article
    let filter_str = filters.join(",");

    let url = format!(
        "{}/works?page={}&per-page={}&mailto={}&filter={}",
        OPENALEX_API_BASE,
        page,
        MAX_PER_PAGE,
        POLITE_EMAIL,
        filter_str
    );

    // Select all needed fields
    let url = format!("{}&select=id,title,display_name,publication_year,publication_date,doi,cited_by_count,abstract_inverted_index,authorships,primary_location,best_oa_location,open_access,language,type,keywords,primary_topic,referenced_works_count,related_works,locations_count", url);

    debug!(url = %url, "Built OpenAlex search URL");

    Ok(url)
}

/// Fetch page content from OpenAlex API
async fn fetch_page(client: &Client, url: &str) -> Result<String> {
    let mut retries = 0;
    let max_retries = 3;

    loop {
        rate_limiter::openalex().acquire().await;
        let response = client.get(url).send().await?;
        let status = response.status();

        if status.is_success() {
            return Ok(response.text().await?);
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if retries < max_retries {
                let backoff = Duration::from_secs(2u64.pow(retries));
                warn!(
                    retries = retries,
                    backoff_secs = backoff.as_secs(),
                    "Rate limited, backing off"
                );
                tokio::time::sleep(backoff).await;
                retries += 1;
                continue;
            }
            return Err(GscholarError::RateLimited(60));
        }

        return Err(GscholarError::Api {
            code: status.as_u16() as i32,
            message: format!("OpenAlex API error: {}", status),
        });
    }
}

/// Parse OpenAlex API response
fn parse_response(json_str: &str) -> Result<Vec<OpenAlexResult>> {
    let response: OpenAlexResponse = serde_json::from_str(json_str)
        .map_err(|e| GscholarError::Parse(format!("Failed to parse OpenAlex response: {}", e)))?;

    let mut results = Vec::new();

    for work in response.results {
        let mut result = OpenAlexResult::default();

        // Title
        result.title = work.display_name
            .or(work.title)
            .unwrap_or_default();

        // OpenAlex ID
        result.openalex_id = work.id.unwrap_or_default();

        // Year
        if let Some(year) = work.publication_year {
            result.year = year.to_string();
        }

        // DOI (clean format without https://doi.org/ prefix)
        if let Some(doi) = work.doi {
            result.doi = doi.replace("https://doi.org/", "");
        }

        // Citations
        if let Some(count) = work.cited_by_count {
            result.citations = count.to_string();
        }

        // Authors (first 3)
        if let Some(authorships) = work.authorships {
            let authors: Vec<String> = authorships
                .iter()
                .take(3)
                .filter_map(|a| a.author.as_ref())
                .filter_map(|a| a.display_name.clone())
                .collect();
            result.author = authors.join(", ");
            if authorships.len() > 3 {
                result.author.push_str(" ...");
            }
        }

        // Venue (journal/source name) and source type
        if let Some(location) = &work.primary_location {
            if let Some(source) = &location.source {
                result.venue = source.display_name.clone().unwrap_or_default();
                result.source_type = source.source_type.clone().unwrap_or_default();
            }
            // Article URL
            result.article_url = location.landing_page_url.clone().unwrap_or_default();
            // PDF URL from primary location
            if let Some(pdf) = &location.pdf_url {
                result.pdf_url = pdf.clone();
            }
        }

        // Best OA location (for PDF and OA URL)
        if let Some(best_oa) = &work.best_oa_location {
            if result.pdf_url.is_empty() {
                result.pdf_url = best_oa.pdf_url.clone().unwrap_or_default();
            }
            if result.article_url.is_empty() {
                result.article_url = best_oa.landing_page_url.clone().unwrap_or_default();
            }
        }

        // Open Access info
        if let Some(oa) = &work.open_access {
            result.is_oa = oa.is_oa.unwrap_or(false);
            result.oa_status = oa.oa_status.clone().unwrap_or_default();
            result.oa_url = oa.oa_url.clone().unwrap_or_default();
            // Fallback URL
            if result.article_url.is_empty() {
                result.article_url = result.oa_url.clone();
            }
        }

        // Abstract (reconstruct from inverted index)
        if let Some(abstract_index) = work.abstract_index {
            result.snippet = reconstruct_abstract(&abstract_index);
        }

        // Publication date
        result.publication_date = work.publication_date.unwrap_or_default();

        // Language
        result.language = work.language.unwrap_or_default();

        // Work type
        result.work_type = work.work_type.unwrap_or_default();

        // Keywords (comma-separated)
        if let Some(keywords) = work.keywords {
            let keyword_list: Vec<String> = keywords
                .into_iter()
                .filter_map(|k| k.display_name)
                .take(5)
                .collect();
            result.keywords = keyword_list.join(", ");
        }

        // Primary topic
        if let Some(topic) = work.primary_topic {
            result.primary_topic = topic.display_name.unwrap_or_default();
        }

        // Referenced works (full list as comma-separated)
        if let Some(refs) = work.referenced_works {
            result.referenced_works = refs.join(",");
            result.referenced_works_count = refs.len() as i64;
        } else {
            result.referenced_works_count = work.referenced_works_count.unwrap_or(0);
        }

        // Related works (full list as comma-separated)
        if let Some(related) = work.related_works {
            result.related_works = related.join(",");
            result.related_works_count = related.len() as i64;
        }

        // Locations count
        result.locations_count = work.locations_count.unwrap_or(0);

        // Only add if we have a title
        if !result.title.is_empty() {
            results.push(result);
        }
    }

    Ok(results)
}

/// Reconstruct abstract text from inverted index
/// OpenAlex provides abstract as inverted index for legal reasons.
/// This function reconstructs the full plaintext abstract.
fn reconstruct_abstract(inverted_index: &serde_json::Value) -> String {
    if let Some(obj) = inverted_index.as_object() {
        // Build (position, word) pairs
        let mut words: Vec<(i64, &str)> = Vec::new();
        
        for (word, positions) in obj {
            if let Some(pos_array) = positions.as_array() {
                for pos in pos_array {
                    if let Some(p) = pos.as_i64() {
                        words.push((p, word.as_str()));
                    }
                }
            }
        }

        // Sort by position and join
        words.sort_by_key(|(pos, _)| *pos);
        words.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_url() {
        let options = QueryOptions {
            pages: vec![1],
            ylo: Some(2020),
            yhi: None,
            all_results: true,
        };
        let current_year = chrono::Utc::now().year();
        
        let url = build_search_url("machine learning", 1, &options).unwrap();
        // Now uses filter=title_and_abstract.search: instead of search=
        assert!(url.contains("title_and_abstract.search:machine+learning"));
        assert!(url.contains("per-page=200"));
        assert!(url.contains("mailto="));
        assert!(url.contains(&format!("publication_year:2020-{}", current_year)));
        assert!(url.contains("type:article"));
    }
}
