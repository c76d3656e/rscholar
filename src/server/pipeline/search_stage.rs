use std::time::Instant;
use tracing::{info, warn};

use crate::server::config::{SearchArxivSection, SearchPubMedSection, SearchXRxivSection};
use crate::{arxiv, openalex, pubmed, semanticscholar, xrxiv};
use futures::stream::{self, StreamExt};

use super::PaperResult;

/// Search stage fan-out result grouped by source semantics.
pub(super) struct SearchStageResult {
    pub ss_results: Vec<PaperResult>,
    pub oa_results: Vec<PaperResult>,
    pub additional_results: Vec<PaperResult>,
    pub failed_sources: Vec<(String, String)>,
    /// Per-source paper counts (before merge/dedup).
    pub source_counts: Vec<(String, usize)>,
}

struct SourceExecutionResult {
    papers: Vec<PaperResult>,
    error: Option<String>,
}

/// Build OpenAlex OR query from original keyword and optional LLM-expanded keywords.
///
/// - If no expansion exists, returns the original keyword.
/// - If expansions exist, combines all terms with `|` (OpenAlex OR syntax).
pub(super) fn build_openalex_or_query(keyword: &str, expanded_keywords: Vec<String>) -> String {
    if expanded_keywords.is_empty() {
        return keyword.to_string();
    }

    let all_terms: Vec<String> = std::iter::once(keyword.to_string())
        .chain(expanded_keywords)
        .collect();

    info!(
        original = keyword,
        oa_terms_count = all_terms.len(),
        oa_query_terms = ?all_terms,
        "Building OpenAlex OR query with expanded keywords"
    );

    all_terms
        .iter()
        .map(|term| {
            if term.contains(' ') {
                format!("\"{}\"", term)
            } else {
                term.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Execute Semantic Scholar + OpenAlex search in parallel and map results
/// into pipeline `PaperResult`.
pub(super) async fn run_parallel_search(
    task_id: &str,
    keyword: &str,
    oa_query: &str,
    ylo: Option<i32>,
    ss_limit: usize,
    oa_limit: usize,
    enabled_sources: &[String],
    arxiv_cfg: &SearchArxivSection,
    pubmed_cfg: &SearchPubMedSection,
    xrxiv_cfg: &SearchXRxivSection,
) -> SearchStageResult {
    let started_at = Instant::now();

    let selected = enabled_sources
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    info!(
        task_id = task_id,
        selected_sources = ?selected,
        "Search stage source selection resolved"
    );

    let source_results: Vec<(String, SourceExecutionResult)> = stream::iter(selected.into_iter())
        .map(|source| async move {
            let source_started = Instant::now();
            let result = match source.as_str() {
                "semanticscholar" => run_semantic_scholar_source(task_id, keyword, ylo, ss_limit).await,
                "openalex" => run_openalex_source(task_id, oa_query, ylo, oa_limit).await,
                "arxiv" => run_arxiv_source(task_id, keyword, ylo, arxiv_cfg).await,
                "pubmed" => run_pubmed_source(task_id, keyword, ylo, pubmed_cfg).await,
                "biorxiv" => run_xrxiv_source(task_id, keyword, xrxiv::XRxivServer::BioRxiv, ylo, xrxiv_cfg).await,
                "medrxiv" => run_xrxiv_source(task_id, keyword, xrxiv::XRxivServer::MedRxiv, ylo, xrxiv_cfg).await,
                other => {
                    warn!(task_id = task_id, source = other, "Search source is not implemented, skipping");
                    SourceExecutionResult {
                        papers: Vec::new(),
                        error: Some("Search source is not implemented".to_string()),
                    }
                }
            };
            info!(
                task_id = task_id,
                source = %source,
                papers = result.papers.len(),
                failed = result.error.is_some(),
                duration_ms = source_started.elapsed().as_millis() as u64,
                "Search source execution complete"
            );
            (source, result)
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let mut stage_result = SearchStageResult {
        ss_results: Vec::new(),
        oa_results: Vec::new(),
        additional_results: Vec::new(),
        failed_sources: Vec::new(),
        source_counts: Vec::new(),
    };

    for (source, result) in source_results {
        // Record per-source count (before merge/dedup)
        stage_result.source_counts.push((source.clone(), result.papers.len()));

        if let Some(err) = result.error {
            stage_result.failed_sources.push((source.clone(), err));
        }

        let papers = result.papers;
        match source.as_str() {
            "semanticscholar" => stage_result.ss_results = papers,
            "openalex" => stage_result.oa_results = papers,
            _ => stage_result.additional_results.extend(papers),
        }
    }

    info!(
        task_id = task_id,
        ss_count = stage_result.ss_results.len(),
        oa_count = stage_result.oa_results.len(),
        additional_count = stage_result.additional_results.len(),
        failed_sources_count = stage_result.failed_sources.len(),
        source_counts = ?stage_result.source_counts,
        duration_ms = started_at.elapsed().as_millis() as u64,
        "Parallel search stage completed"
    );

    stage_result
}

async fn run_arxiv_source(
    task_id: &str,
    keyword: &str,
    ylo: Option<i32>,
    cfg: &SearchArxivSection,
) -> SourceExecutionResult {
    let options = arxiv::ArxivQueryOptions {
        max_results: cfg.max_results.clamp(1, 200),
        page_size: cfg.page_size.clamp(1, 200),
        sort_by: cfg.sort_by.clone(),
        sort_order: cfg.sort_order.clone(),
        timeout_secs: cfg.timeout_sec,
        request_delay_ms: cfg.request_delay_ms,
    };
    match arxiv::search_papers(keyword, &options).await {
        Ok(results) => {
            let fetched_count = results.len();
            let filtered_results = if let Some(year_from) = ylo {
                results
                    .into_iter()
                    .filter(|r| r.year.parse::<i32>().map(|y| y >= year_from).unwrap_or(false))
                    .collect::<Vec<_>>()
            } else {
                results
            };
            info!(
                task_id = task_id,
                source = "arxiv",
                ylo = ?ylo,
                fetched = fetched_count,
                papers = filtered_results.len(),
                max_results = options.max_results,
                page_size = options.page_size,
                "arXiv search complete"
            );
            SourceExecutionResult {
                papers: filtered_results
                .into_iter()
                .map(|r| PaperResult {
                    title: r.title,
                    authors: r.authors,
                    year: r.year,
                    venue: "arXiv".to_string(),
                    doi: r.doi,
                    url: r.url,
                    pdf_url: r.pdf_url,
                    snippet: String::new(),
                    abstract_text: r.abstract_text,
                    source: "arxiv".to_string(),
                    ..Default::default()
                })
                .collect(),
                error: None,
            }
        }
        Err(error) => {
            warn!(
                task_id = task_id,
                source = "arxiv",
                error = %error,
                "arXiv search failed"
            );
            SourceExecutionResult {
                papers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    }
}

async fn run_pubmed_source(
    task_id: &str,
    keyword: &str,
    ylo: Option<i32>,
    cfg: &SearchPubMedSection,
) -> SourceExecutionResult {
    let query = if let Some(year_from) = ylo {
        format!("({keyword}) AND {year_from}:3000[pdat]")
    } else {
        keyword.to_string()
    };
    info!(
        task_id = task_id,
        source = "pubmed",
        ylo = ?ylo,
        query = %query,
        "Prepared PubMed query with year filter"
    );

    let options = pubmed::PubMedQueryOptions {
        max_results: cfg.max_results.clamp(1, 200),
        page_size: cfg.page_size.clamp(1, 200),
        timeout_secs: cfg.timeout_sec,
        api_key: if cfg.api_key.trim().is_empty() {
            None
        } else {
            Some(cfg.api_key.clone())
        },
        tool: if cfg.tool.trim().is_empty() {
            None
        } else {
            Some(cfg.tool.clone())
        },
        email: if cfg.email.trim().is_empty() {
            None
        } else {
            Some(cfg.email.clone())
        },
        delay_no_key_ms: cfg.delay_no_key_ms,
        delay_with_key_ms: cfg.delay_with_key_ms,
    };

    match pubmed::search_papers(&query, &options).await {
        Ok(results) => {
            info!(
                task_id = task_id,
                source = "pubmed",
                papers = results.len(),
                max_results = options.max_results,
                page_size = options.page_size,
                "PubMed search complete"
            );
            SourceExecutionResult {
                papers: results
                .into_iter()
                .map(|r| PaperResult {
                    title: r.title,
                    authors: r.authors,
                    year: r.year,
                    venue: r.venue,
                    doi: r.doi,
                    url: r.url,
                    pdf_url: String::new(),
                    snippet: String::new(),
                    abstract_text: r.abstract_text,
                    source: "pubmed".to_string(),
                    ..Default::default()
                })
                .collect(),
                error: None,
            }
        }
        Err(error) => {
            warn!(
                task_id = task_id,
                source = "pubmed",
                error = %error,
                "PubMed search failed"
            );
            SourceExecutionResult {
                papers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    }
}

async fn run_xrxiv_source(
    task_id: &str,
    keyword: &str,
    server: xrxiv::XRxivServer,
    ylo: Option<i32>,
    cfg: &SearchXRxivSection,
) -> SourceExecutionResult {
    let max_results = match server {
        xrxiv::XRxivServer::BioRxiv => cfg.biorxiv_max_results.clamp(1, 200),
        xrxiv::XRxivServer::MedRxiv => cfg.medrxiv_max_results.clamp(1, 200),
    };

    let start_date = if let Some(year_from) = ylo {
        format!("{year_from}-01-01")
    } else {
        cfg.start_date.clone()
    };
    let end_date = if cfg.end_date.trim().is_empty() {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    } else {
        cfg.end_date.clone()
    };

    let options = xrxiv::XRxivQueryOptions {
        max_results,
        start_date,
        end_date,
        timeout_secs: cfg.timeout_sec,
        request_delay_ms: cfg.request_delay_ms,
        max_retries: cfg.max_retries,
    };
    info!(
        task_id = task_id,
        source = %server.as_path(),
        ylo = ?ylo,
        start_date = %options.start_date,
        end_date = %options.end_date,
        "Prepared xrxiv date range with year filter"
    );

    match xrxiv::search_papers(server, keyword, &options).await {
        Ok(results) => {
            info!(
                task_id = task_id,
                source = %server.as_path(),
                papers = results.len(),
                max_results = max_results,
                "xrxiv search complete"
            );
            SourceExecutionResult {
                papers: results
                .into_iter()
                .map(|r| PaperResult {
                    title: r.title,
                    authors: r.authors,
                    year: r.year,
                    venue: r.venue.clone(),
                    doi: r.doi,
                    url: r.url,
                    pdf_url: r.pdf_url,
                    snippet: String::new(),
                    abstract_text: r.abstract_text,
                    source: r.venue.to_lowercase(),
                    ..Default::default()
                })
                .collect(),
                error: None,
            }
        }
        Err(error) => {
            warn!(
                task_id = task_id,
                source = %server.as_path(),
                error = %error,
                "xrxiv search failed"
            );
            SourceExecutionResult {
                papers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    }
}

async fn run_semantic_scholar_source(
    task_id: &str,
    keyword: &str,
    ylo: Option<i32>,
    ss_limit: usize,
) -> SourceExecutionResult {
    match semanticscholar::search_papers(keyword, ylo, ss_limit, None).await {
        Ok(results) => {
            info!(
                task_id = task_id,
                papers = results.len(),
                keyword = keyword,
                ss_limit = ss_limit,
                "Semantic Scholar search complete"
            );
            SourceExecutionResult {
                papers: results
                .into_iter()
                .map(|r| PaperResult {
                    title: r.title,
                    authors: r.authors,
                    year: r.year,
                    venue: r.venue,
                    doi: r.doi,
                    url: r.url,
                    pdf_url: r.pdf_url,
                    snippet: String::new(),
                    abstract_text: r.ss_abstract,
                    source: "semanticscholar".to_string(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
                error: None,
            }
        }
        Err(error) => {
            warn!(
                task_id = task_id,
                keyword = keyword,
                ss_limit = ss_limit,
                error = %error,
                "Semantic Scholar search failed"
            );
            SourceExecutionResult {
                papers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    }
}

async fn run_openalex_source(
    task_id: &str,
    oa_query: &str,
    ylo: Option<i32>,
    oa_limit: usize,
) -> SourceExecutionResult {
    let options = openalex::QueryOptions {
        pages: vec![1],
        ylo,
        ..Default::default()
    };
    info!(task_id = task_id, query = oa_query, oa_limit = oa_limit, "Starting OpenAlex search");
    match openalex::query(oa_query, &options).await {
        Ok(results) => {
            let selected = results.into_iter().take(oa_limit).collect::<Vec<_>>();
            info!(
                task_id = task_id,
                papers = selected.len(),
                query = oa_query,
                oa_limit = oa_limit,
                "OpenAlex search complete"
            );
            SourceExecutionResult {
                papers: selected
                .into_iter()
                .map(|r| PaperResult {
                    title: r.title,
                    authors: r.author,
                    year: r.year,
                    venue: r.venue,
                    doi: r.doi,
                    url: r.article_url,
                    pdf_url: r.pdf_url,
                    snippet: r.snippet.clone(),
                    abstract_text: r.snippet,
                    source: "openalex".to_string(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
                error: None,
            }
        }
        Err(error) => {
            warn!(
                task_id = task_id,
                query = oa_query,
                oa_limit = oa_limit,
                error = %error,
                "OpenAlex search failed"
            );
            SourceExecutionResult {
                papers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    }
}
