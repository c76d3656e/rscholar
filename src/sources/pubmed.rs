//! PubMed E-utilities client.
//!
//! Search flow:
//! 1) `esearch.fcgi` to retrieve PMID list
//! 2) `efetch.fcgi` to retrieve article metadata and abstracts
//!
//! This module returns normalized records and includes conservative throttling.

use crate::error::Result;
use crate::sources::rate_limiter;
use crate::sources::{SourcePaper, SourceProvider};
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

const PUBMED_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const PUBMED_MAX_RETMAX: usize = 10_000;
const PUBMED_EFETCH_BATCH_SIZE: usize = 200;

/// PubMed query options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PubMedQueryOptions {
    pub max_results: usize,
    pub page_size: usize,
    pub timeout_secs: u64,
    pub api_key: Option<String>,
    pub tool: Option<String>,
    pub email: Option<String>,
    /// Delay between calls when no API key (3 RPS => ~333ms).
    pub delay_no_key_ms: u64,
    /// Delay between calls with API key (10 RPS => ~100ms).
    pub delay_with_key_ms: u64,
}

impl PubMedQueryOptions {
    pub fn with_defaults() -> Self {
        Self {
            max_results: 100,
            page_size: 100,
            timeout_secs: 30,
            api_key: None,
            tool: Some("Rscholar".to_string()),
            email: Some("c76d@c.com".to_string()),
            delay_no_key_ms: 350,
            delay_with_key_ms: 120,
        }
    }
}

/// PubMed normalized result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PubMedResult {
    pub title: String,
    pub authors: String,
    pub year: String,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub abstract_text: String,
}

#[derive(Debug, Deserialize)]
struct ESearchResponse {
    esearchresult: ESearchResult,
}

#[derive(Debug, Deserialize)]
struct ESearchResult {
    count: String,
    idlist: Vec<String>,
}

/// PubMed provider implementing unified source trait.
pub struct PubMedProvider {
    client: Client,
}

impl PubMedProvider {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("Rscholar/0.1 (pubmed source)")
            .build()?;
        Ok(Self { client })
    }
}

#[async_trait]
impl SourceProvider<PubMedQueryOptions> for PubMedProvider {
    fn source_name(&self) -> &'static str {
        "pubmed"
    }

    async fn search(&self, query: &str, options: &PubMedQueryOptions) -> Result<Vec<SourcePaper>> {
        let papers = search_with_client(&self.client, query, options).await?;
        Ok(papers
            .into_iter()
            .map(|p| SourcePaper {
                title: p.title,
                authors: p.authors,
                year: p.year,
                venue: p.venue,
                doi: p.doi,
                url: p.url,
                pdf_url: String::new(),
                snippet: String::new(),
                abstract_text: p.abstract_text,
            })
            .collect())
    }
}

/// Search PubMed and return normalized results with abstracts.
pub async fn search_papers(query: &str, options: &PubMedQueryOptions) -> Result<Vec<PubMedResult>> {
    let provider = PubMedProvider::new(options.timeout_secs)?;
    search_with_client(&provider.client, query, options).await
}

async fn search_with_client(
    client: &Client,
    query: &str,
    options: &PubMedQueryOptions,
) -> Result<Vec<PubMedResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let max_results = options.max_results.max(1);
    let page_size = options.page_size.clamp(1, PUBMED_MAX_RETMAX);
    let has_key = options.api_key.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);


    info!(
        query = query,
        max_results = max_results,
        page_size = page_size,
        has_api_key = has_key,
        "Starting PubMed search"
    );

    let mut all_pmids: Vec<String> = Vec::new();
    let mut retstart = 0usize;

    while all_pmids.len() < max_results {
        let remaining = max_results - all_pmids.len();
        let retmax = remaining.min(page_size);
        let search_resp = call_esearch(client, query, retstart, retmax, options).await?;
        let mut page_ids = search_resp.esearchresult.idlist;
        let total_count: usize = search_resp.esearchresult.count.parse().unwrap_or(0);

        let fetched = page_ids.len();
        info!(
            retstart = retstart,
            retmax = retmax,
            fetched = fetched,
            total_count = total_count,
            accumulated = all_pmids.len() + fetched,
            "PubMed ESearch page fetched"
        );
        all_pmids.append(&mut page_ids);

        if fetched == 0 || fetched < retmax || all_pmids.len() >= total_count {
            break;
        }

        retstart += retmax;
    }

    all_pmids.truncate(max_results);
    if all_pmids.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for pmid_chunk in all_pmids.chunks(PUBMED_EFETCH_BATCH_SIZE) {
        let xml = call_efetch(client, pmid_chunk, options).await?;
        let chunk_results = parse_pubmed_xml(&xml);
        info!(
            requested_pmids = pmid_chunk.len(),
            parsed_records = chunk_results.len(),
            "PubMed EFetch batch parsed"
        );
        out.extend(chunk_results);
    }

    out.truncate(max_results);
    info!(total = out.len(), "PubMed search completed");
    Ok(out)
}

async fn call_esearch(
    client: &Client,
    query: &str,
    retstart: usize,
    retmax: usize,
    options: &PubMedQueryOptions,
) -> Result<ESearchResponse> {
    let has_key = options.api_key.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);

    let mut rate_limit_retries = 0u32;
    let mut server_error_retries = 0u32;

    let response = loop {
        let mut req = client.get(format!("{PUBMED_BASE_URL}/esearch.fcgi")).query(&[
            ("db", "pubmed"),
            ("term", query),
            ("retmode", "json"),
            ("retstart", &retstart.to_string()),
            ("retmax", &retmax.to_string()),
        ]);

        if let Some(v) = options.api_key.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("api_key", v.as_str())]);
        }
        if let Some(v) = options.tool.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("tool", v.as_str())]);
        }
        if let Some(v) = options.email.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("email", v.as_str())]);
        }

        rate_limiter::pubmed(has_key).acquire().await;
        let resp = req.send().await?;
        let status = resp.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            rate_limit_retries += 1;
            let backoff_secs = 1u64 << rate_limit_retries.min(4); // 1, 2, 4, 8, 16s
            warn!(
                source = "pubmed",
                status = 429,
                attempt = rate_limit_retries,
                backoff_secs = backoff_secs,
                "PubMed rate limited (429), retrying with exponential backoff"
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            continue;
        }

        if status.is_server_error() {
            server_error_retries += 1;
            if server_error_retries > 5 {
                warn!(source = "pubmed", status = %status, "PubMed server error after 5 retries, giving up");
                break resp;
            }
            warn!(
                source = "pubmed",
                status = %status,
                attempt = server_error_retries,
                "PubMed server error, retrying in 2s"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        break resp;
    };

    let response = response.error_for_status()?;
    let parsed = response.json::<ESearchResponse>().await?;
    Ok(parsed)
}

async fn call_efetch(
    client: &Client,
    pmids: &[String],
    options: &PubMedQueryOptions,
) -> Result<String> {
    let joined = pmids.join(",");
    let has_key = options.api_key.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);

    let mut rate_limit_retries = 0u32;
    let mut server_error_retries = 0u32;

    let response = loop {
        let mut req = client.get(format!("{PUBMED_BASE_URL}/efetch.fcgi")).query(&[
            ("db", "pubmed"),
            ("id", joined.as_str()),
            ("retmode", "xml"),
        ]);

        if let Some(v) = options.api_key.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("api_key", v.as_str())]);
        }
        if let Some(v) = options.tool.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("tool", v.as_str())]);
        }
        if let Some(v) = options.email.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.query(&[("email", v.as_str())]);
        }

        rate_limiter::pubmed(has_key).acquire().await;
        let resp = req.send().await?;
        let status = resp.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            rate_limit_retries += 1;
            let backoff_secs = 1u64 << rate_limit_retries.min(4);
            warn!(
                source = "pubmed",
                status = 429,
                attempt = rate_limit_retries,
                backoff_secs = backoff_secs,
                pmid_count = pmids.len(),
                "PubMed efetch rate limited (429), retrying with exponential backoff"
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            continue;
        }

        if status.is_server_error() {
            server_error_retries += 1;
            if server_error_retries > 5 {
                warn!(source = "pubmed", status = %status, "PubMed efetch server error after 5 retries, giving up");
                break resp;
            }
            warn!(
                source = "pubmed",
                status = %status,
                attempt = server_error_retries,
                "PubMed efetch server error, retrying in 2s"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        break resp;
    };

    let text = response.error_for_status()?.text().await?;
    Ok(text)
}

fn parse_pubmed_xml(xml: &str) -> Vec<PubMedResult> {
    let article_re = regex_or_empty(r"(?s)<PubmedArticle>(.*?)</PubmedArticle>");
    let pmid_re = regex_or_empty(r"(?s)<PMID[^>]*>(.*?)</PMID>");
    let title_re = regex_or_empty(r"(?s)<ArticleTitle>(.*?)</ArticleTitle>");
    let abstract_re = regex_or_empty(r"(?s)<AbstractText[^>]*>(.*?)</AbstractText>");
    let journal_re = regex_or_empty(r"(?s)<Title>(.*?)</Title>");
    let year_re = regex_or_empty(r"(?s)<PubDate>.*?<Year>(\d{4})</Year>.*?</PubDate>");
    let doi_re = regex_or_empty(r#"(?s)<ArticleId IdType="doi">(.*?)</ArticleId>"#);
    let author_re = regex_or_empty(r"(?s)<Author[^>]*>(.*?)</Author>");
    let lastname_re = regex_or_empty(r"(?s)<LastName>(.*?)</LastName>");
    let forename_re = regex_or_empty(r"(?s)<ForeName>(.*?)</ForeName>");

    article_re
        .captures_iter(xml)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .map(|article| {
            let pmid = capture_first(&pmid_re, article).unwrap_or_default();
            let title = capture_first(&title_re, article).unwrap_or_default();
            let mut abstract_parts = Vec::new();
            for cap in abstract_re.captures_iter(article) {
                if let Some(m) = cap.get(1) {
                    let part = decode_xml_entities(strip_tags(m.as_str()).trim());
                    if !part.is_empty() {
                        abstract_parts.push(part);
                    }
                }
            }

            let authors = author_re
                .captures_iter(article)
                .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
                .map(|author_block| {
                    let last = capture_first(&lastname_re, author_block).unwrap_or_default();
                    let first = capture_first(&forename_re, author_block).unwrap_or_default();
                    if first.is_empty() {
                        last
                    } else {
                        format!("{first} {last}")
                    }
                })
                .filter(|name| !name.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ");

            let year = capture_first(&year_re, article).unwrap_or_default();
            let doi = capture_first(&doi_re, article).unwrap_or_default();
            let venue = capture_first(&journal_re, article).unwrap_or_default();

            let abstract_text = abstract_parts.join(" ");
            let clean_title = decode_xml_entities(strip_tags(title.as_str()).trim());

            PubMedResult {
                title: clean_title,
                authors,
                year,
                venue,
                doi,
                url: if pmid.is_empty() {
                    String::new()
                } else {
                    format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/")
                },
                abstract_text,
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
    regex_or_empty(r"(?s)<[^>]+>").replace_all(text, "").to_string()
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

fn regex_or_empty(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| {
        warn!(pattern = pattern, error = %error, "Regex compile failed, using empty regex");
        Regex::new("$^").unwrap_or_else(|fallback_error| {
            debug!(error = %fallback_error, "Fallback regex compile unexpectedly failed");
            Regex::new(r"a^").unwrap_or_else(|_| panic!("failed to create fallback regex"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pubmed_esearch_raw_response() {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Rscholar/0.1 (pubmed test)")
            .build()
            .expect("failed to build client");

        let query = "(Diabetes) AND 2021:3000[pdat]";
        let url = format!("{PUBMED_BASE_URL}/esearch.fcgi");

        let mut req = client.get(&url).query(&[
            ("db", "pubmed"),
            ("term", query),
            ("retmode", "json"),
            ("retstart", "0"),
            ("retmax", "5"),
        ]);

        // Include API key from env if available
        if let Ok(api_key) = std::env::var("PUBMED_API_KEY") {
            if !api_key.trim().is_empty() {
                req = req.query(&[("api_key", api_key.as_str())]);
            }
        }

        let resp = req.send().await.expect("request failed");
        let status = resp.status();
        let headers = format!("{:?}", resp.headers());
        let body = resp.text().await.expect("failed to read body text");

        println!("=== PubMed ESearch Raw Response ===");
        println!("Status: {status}");
        println!("Headers: {headers}");
        println!("Body length: {} bytes", body.len());
        println!("Body (first 2000 chars):\n{}", &body[..body.len().min(2000)]);
        println!("=== End Raw Response ===");

        // Try parsing as JSON to see the actual error
        match serde_json::from_str::<ESearchResponse>(&body) {
            Ok(parsed) => {
                println!("Parsed OK: count={}, idlist len={}", 
                    parsed.esearchresult.count, parsed.esearchresult.idlist.len());
            }
            Err(e) => {
                println!("JSON parse error: {e}");
                println!("Full body:\n{body}");
            }
        }
    }

    #[test]
    fn test_parse_pubmed_xml_minimal() {
        let xml = r#"
<PubmedArticleSet>
  <PubmedArticle>
    <MedlineCitation>
      <PMID>12345678</PMID>
      <Article>
        <ArticleTitle>Test Article</ArticleTitle>
        <Abstract>
          <AbstractText>Abstract part one.</AbstractText>
          <AbstractText>Part two.</AbstractText>
        </Abstract>
        <Journal>
          <Title>Example Journal</Title>
          <JournalIssue>
            <PubDate><Year>2023</Year></PubDate>
          </JournalIssue>
        </Journal>
        <AuthorList>
          <Author><LastName>Smith</LastName><ForeName>Alice</ForeName></Author>
          <Author><LastName>Jones</LastName><ForeName>Bob</ForeName></Author>
        </AuthorList>
      </Article>
    </MedlineCitation>
    <PubmedData>
      <ArticleIdList>
        <ArticleId IdType="doi">10.1000/example</ArticleId>
      </ArticleIdList>
    </PubmedData>
  </PubmedArticle>
</PubmedArticleSet>
        "#;

        let parsed = parse_pubmed_xml(xml);
        assert_eq!(parsed.len(), 1);
        let p = &parsed[0];
        assert_eq!(p.year, "2023");
        assert_eq!(p.doi, "10.1000/example");
        assert!(p.abstract_text.contains("part one"));
        assert!(p.url.contains("12345678"));
    }
}
