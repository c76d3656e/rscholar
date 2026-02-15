//! Pipeline execution for async API.
//!
//! Refactored pipeline logic for use in HTTP handlers.
//! Includes throttled progress updates to avoid SQLite write lock contention.

use crate::db::{tasks as db_tasks, DbPool};
use crate::error::{GscholarError, Result};
use crate::server::config::{SearchArxivSection, SearchPubMedSection, SearchXRxivSection};
use crate::server::task::{TaskResult, TaskStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};

mod analytics;
mod export;
mod fallback;
mod keyword_expansion;
mod keyword_translation;
mod llm_filter;
mod merge;
mod search_stage;

use analytics::log_search_analytics;
use export::save_results_csv;
use fallback::select_results_with_fallback;
use keyword_expansion::run_keyword_expansion;
use keyword_translation::run_keyword_translation;
use llm_filter::apply_llm_relevance_filter;
use merge::merge_search_results;
use search_stage::{build_openalex_or_query, run_parallel_search};

/// Pipeline request from API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineRequest {
    /// Search keywords
    pub keyword: String,
    /// Year lower bound filter
    pub ylo: Option<i32>,
    /// Enable Crossref metadata enrichment
    pub enable_crossref: Option<bool>,

    /// IF score filter
    pub sciif: Option<f64>,
    /// JCI filter
    pub jci: Option<f64>,
    /// SCI partition filter
    pub sci: Option<String>,
    /// Strict mode: if true, do NOT fallback to unfiltered results when LLM filters all papers out
    pub llm_strict_filter: Option<bool>,
    /// Content help: User's description of desired research direction/focus.
    /// Used for BOTH keyword expansion and LLM relevance filtering.
    /// Example: "我需要关于机器学习预测岩石强度的论文，重点关注实时方法"
    pub content_help: Option<String>,
    /// Optional source include list (case-insensitive), e.g. ["openalex","semanticscholar"].
    pub source_include: Option<Vec<String>>,
    /// Optional source exclude list (case-insensitive).
    pub source_exclude: Option<Vec<String>>,
}

impl PipelineRequest {
    /// Validate the request
    pub fn validate(&self) -> Result<()> {
        if self.keyword.trim().is_empty() {
            return Err(GscholarError::Validation("Missing required parameter 'keyword' in JSON body.".to_string()));
        }
        Ok(())
    }
}

/// Pipeline configuration merged from request and server config
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub keyword: String,
    pub ylo: Option<i32>,
    pub enable_crossref: bool,
    pub ss_limit: usize,
    pub oa_limit: usize,
    pub enabled_sources: Vec<String>,
    pub arxiv: SearchArxivSection,
    pub pubmed: SearchPubMedSection,
    pub xrxiv: SearchXRxivSection,

    pub easyscholar_keys: Vec<String>,
    pub sciif: Option<f64>,
    pub jci: Option<f64>,
    pub sci: Option<String>,
    
    pub llm_strict_filter: bool,
    /// User's description for keyword expansion and relevance filtering
    pub content_help: Option<String>,
    pub output_dir: PathBuf,
}

impl PipelineConfig {
    /// Create config from request and server settings
    pub fn from_request(
        req: PipelineRequest,
        server_easyscholar_keys: &[String],
        _server_llm_enabled: bool,
        server_llm_strict_filter: bool,
        server_default_ylo: Option<i32>,
        server_enable_crossref: bool,
        server_ss_limit: usize,
        server_oa_limit: usize,
        server_enabled_sources: &[String],
        server_arxiv: &SearchArxivSection,
        server_pubmed: &SearchPubMedSection,
        server_xrxiv: &SearchXRxivSection,
    ) -> Result<Self> {
        req.validate()?;

        // Create output directory with timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let sanitized_keyword = req
            .keyword
            .chars()
            .take(50)
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();
        let output_dir = PathBuf::from(format!("output/{}_{}", timestamp, sanitized_keyword));

        let easyscholar_keys = server_easyscholar_keys.to_vec();

        let filter_active = req.sciif.is_some()
            || req.jci.is_some()
            || req.sci.is_some();
        
        if filter_active && easyscholar_keys.is_empty() {
             return Err(crate::error::GscholarError::Validation(
                 "Ranking filters (sciif/jci/sci) require EasyScholar access. Please configure server-side easyscholar.keys.".to_string()
             ));
        }

        let enabled_sources = resolve_enabled_sources(
            req.source_include.clone(),
            req.source_exclude.clone(),
            server_enabled_sources,
        )?;

        Ok(Self {
            keyword: req.keyword,
            ylo: req.ylo.or(server_default_ylo),
            enable_crossref: req.enable_crossref.unwrap_or(server_enable_crossref),
            ss_limit: server_ss_limit.clamp(1, 100),
            oa_limit: server_oa_limit.clamp(1, 200),
            enabled_sources,
            arxiv: server_arxiv.clone(),
            pubmed: server_pubmed.clone(),
            xrxiv: server_xrxiv.clone(),

            easyscholar_keys,
            sciif: req.sciif,
            jci: req.jci,
            sci: req.sci,
            
            llm_strict_filter: req.llm_strict_filter.unwrap_or(server_llm_strict_filter),
            content_help: req.content_help,
            output_dir,
        })
    }
}

fn normalize_source_name(value: &str) -> String {
    value.trim().to_lowercase()
}

fn resolve_enabled_sources(
    source_include: Option<Vec<String>>,
    source_exclude: Option<Vec<String>>,
    server_enabled_sources: &[String],
) -> Result<Vec<String>> {
    use std::collections::HashSet;

    const ALLOWED_SOURCES: [&str; 6] = [
        "openalex",
        "semanticscholar",
        "arxiv",
        "pubmed",
        "biorxiv",
        "medrxiv",
    ];
    let allowed_set: HashSet<&str> = ALLOWED_SOURCES.into_iter().collect();

    let base: Vec<String> = match source_include {
        Some(values) if !values.is_empty() => values,
        _ => server_enabled_sources.to_vec(),
    };

    let mut names: Vec<String> = base
        .into_iter()
        .map(|v| normalize_source_name(&v))
        .filter(|v| !v.is_empty())
        .collect();

    let unknown: Vec<String> = names
        .iter()
        .filter(|name| !allowed_set.contains(name.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(GscholarError::Validation(format!(
            "Unsupported source(s): {}. Supported sources: {}",
            unknown.join(", "),
            ALLOWED_SOURCES.join(", ")
        )));
    }

    if names.is_empty() {
        names = vec!["openalex".to_string(), "semanticscholar".to_string()];
    }

    let exclude_set: HashSet<String> = source_exclude
        .unwrap_or_default()
        .into_iter()
        .map(|v| normalize_source_name(&v))
        .filter(|v| !v.is_empty())
        .collect();

    let mut seen = HashSet::new();
    let resolved: Vec<String> = names
        .into_iter()
        .filter(|name| !exclude_set.contains(name))
        .filter(|name| seen.insert(name.clone()))
        .collect();
    Ok(resolved)
}

/// Progress tracker with throttling for DB writes
/// 
/// Writes to memory on every update, but throttles DB writes to avoid
/// SQLite write lock contention. DB writes occur when:
/// - Progress changes by >= 5%
/// - Time since last write >= 2 seconds
/// - Status transitions (always written immediately)
pub struct ProgressTracker {
    task_id: String,
    last_db_percent: u8,
    last_db_time: Instant,
    task_store: TaskStore,
    db: Option<DbPool>,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(task_id: String, task_store: TaskStore, db: Option<DbPool>) -> Self {
        Self {
            task_id,
            last_db_percent: 0,
            last_db_time: Instant::now(),
            task_store,
            db,
        }
    }

    /// Update progress - always updates memory, throttles DB writes
    pub async fn update(&mut self, step: &str, percent: u8) {
        // Always update memory (fast)
        self.task_store.update(&self.task_id, |t| {
            t.update_progress(step, percent);
        });

        // Throttle DB writes: Δ >= 5% OR Δt >= 2s
        let delta_percent = percent.saturating_sub(self.last_db_percent);
        let delta_time = self.last_db_time.elapsed().as_secs();
        
        if delta_percent >= 5 || delta_time >= 2 {
            self.write_progress_to_db(step, percent).await;
        }
    }

    /// Force DB write (for status transitions like start/complete/fail)
    pub async fn force_update(&mut self, step: &str, percent: u8) {
        // Update memory
        self.task_store.update(&self.task_id, |t| {
            t.update_progress(step, percent);
        });
        
        // Force DB write
        self.write_progress_to_db(step, percent).await;
    }

    /// Internal: write progress to database
    async fn write_progress_to_db(&mut self, step: &str, percent: u8) {
        if let Some(ref db) = self.db {
            match db.get().await {
                Ok(conn) => {
                    let task_id = self.task_id.clone();
                    let step_owned = step.to_string();
                    match conn.interact(move |conn| {
                        db_tasks::update_progress(conn, &task_id, &step_owned, percent)
                    }).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            warn!(
                                task_id = %self.task_id,
                                step = %step,
                                percent = percent,
                                error = %error,
                                "Failed to persist task progress to database"
                            );
                        }
                        Err(error) => {
                            warn!(
                                task_id = %self.task_id,
                                step = %step,
                                percent = percent,
                                error = %error,
                                "DB interact failed when persisting task progress"
                            );
                        }
                    }
                }
                Err(error) => {
                    warn!(
                        task_id = %self.task_id,
                        step = %step,
                        percent = percent,
                        error = %error,
                        "Failed to get DB connection for task progress persistence"
                    );
                }
            }
        }
        self.last_db_percent = percent;
        self.last_db_time = Instant::now();
    }

    /// Complete the task - updates both memory and DB immediately
    pub async fn complete(&self, result: TaskResult) {
        // Update memory
        self.task_store.update(&self.task_id, |t| t.complete(result.clone()));

        // Persist to DB
        if let Some(ref db) = self.db {
            match db.get().await {
                Ok(conn) => {
                    let task_id = self.task_id.clone();
                    let db_result = db_tasks::TaskResult {
                        total_papers: result.total_papers,
                        filtered_papers: result.filtered_papers,
                        data: result.data.clone(),
                        csv_path: result.csv_path.clone(),
                    };
                    match conn.interact(move |conn| {
                        db_tasks::complete(conn, &task_id, &db_result)
                    }).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            warn!(
                                task_id = %self.task_id,
                                error = %error,
                                "Failed to persist completed task result to database"
                            );
                        }
                        Err(error) => {
                            warn!(
                                task_id = %self.task_id,
                                error = %error,
                                "DB interact failed when persisting completed task"
                            );
                        }
                    }
                }
                Err(error) => {
                    warn!(
                        task_id = %self.task_id,
                        error = %error,
                        "Failed to get DB connection for completed task persistence"
                    );
                }
            }
        }
    }

    /// Fail the task - updates both memory and DB immediately
    pub async fn fail(&self, error: &str) {
        // Update memory
        self.task_store.update(&self.task_id, |t| t.fail(error.to_string()));

        // Persist to DB
        if let Some(ref db) = self.db {
            match db.get().await {
                Ok(conn) => {
                    let task_id = self.task_id.clone();
                    let error_owned = error.to_string();
                    match conn.interact(move |conn| {
                        db_tasks::fail(conn, &task_id, &error_owned)
                    }).await {
                        Ok(Ok(())) => {}
                        Ok(Err(db_error)) => {
                            warn!(
                                task_id = %self.task_id,
                                error = %db_error,
                                "Failed to persist failed task status to database"
                            );
                        }
                        Err(interact_error) => {
                            warn!(
                                task_id = %self.task_id,
                                error = %interact_error,
                                "DB interact failed when persisting failed task"
                            );
                        }
                    }
                }
                Err(pool_error) => {
                    warn!(
                        task_id = %self.task_id,
                        error = %pool_error,
                        "Failed to get DB connection for failed task persistence"
                    );
                }
            }
        }
    }

    /// Get the task ID
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
}

/// Spawn pipeline execution as a background task
/// 
/// Uses ProgressTracker for throttled DB progress updates.
/// On completion/failure, persists result to DB and removes task from memory cache
/// after a delay (allowing immediate result retrieval).
pub fn spawn_pipeline(
    task_store: TaskStore,
    task_id: String,
    config: PipelineConfig,
    db: Option<DbPool>,
    llm_filter: Option<std::sync::Arc<crate::llm::LlmRelevanceFilter>>,
    ranking_service: Option<std::sync::Arc<crate::rankings::RankingService>>,
) {
    tokio::spawn(async move {
        info!(task_id = %task_id, keyword = %config.keyword, "Starting pipeline execution");
        
        // Create progress tracker with throttled DB writes
        let mut tracker = ProgressTracker::new(
            task_id.clone(),
            task_store.clone(),
            db.clone(),
        );

        // Force initial progress to DB (status transition: pending -> running)
        tracker.force_update("Starting search", 5).await;

        match execute_pipeline(
            &mut tracker,
            config,
            db.as_ref(),
            llm_filter,
            ranking_service,
        )
        .await
        {
            Ok(result) => {
                info!(task_id = %task_id, papers = result.total_papers, "Pipeline completed");
                tracker.complete(result).await;
            }
            Err(e) => {
                error!(task_id = %task_id, error = %e, "Pipeline failed");
                tracker.fail(&e.to_string()).await;
            }
        }

        // Note: Task remains in memory for immediate polling.
        // Background cleanup job will remove it after TTL expires.
    });
}

/// Execute the full pipeline (internal implementation)
async fn execute_pipeline(
    tracker: &mut ProgressTracker,
    config: PipelineConfig,
    db: Option<&DbPool>,
    llm_filter: Option<std::sync::Arc<crate::llm::LlmRelevanceFilter>>,
    ranking_service: Option<std::sync::Arc<crate::rankings::RankingService>>,
) -> Result<TaskResult> {
    use crate::{crossref, semanticscholar};

    // Clone task_id to owned String to avoid borrow conflict with tracker.update()
    let task_id = tracker.task_id().to_string();

    // Create output directory
    std::fs::create_dir_all(&config.output_dir)?;

    // Stage 0: Keyword Translation (if non-English input and LLM available)
    // All downstream search/expansion uses the translated English keyword.
    let search_keyword = run_keyword_translation(tracker, &config.keyword, llm_filter.as_ref()).await;
    info!(
        task_id = %task_id,
        original_keyword = %config.keyword,
        search_keyword = %search_keyword,
        "Keyword prepared for search pipeline"
    );

    // Stage 0.5: Keyword Expansion (based on translated English keyword)
    let expanded_keywords =
        run_keyword_expansion(tracker, &config, llm_filter.as_ref(), &search_keyword).await;

    // Stage 1: Search enabled sources in parallel.
    tracker.update("Searching papers", 10).await;
    
    // Build OpenAlex OR query from original + expanded terms.
    let oa_query = build_openalex_or_query(&search_keyword, expanded_keywords);
    
    // Parallel search
    let keyword = search_keyword.clone();
    let ylo = config.ylo;
    let ss_limit = config.ss_limit;
    let oa_limit = config.oa_limit;
    
    let stage_result = run_parallel_search(
        &task_id,
        &keyword,
        &oa_query,
        ylo,
        ss_limit,
        oa_limit,
        &config.enabled_sources,
        &config.arxiv,
        &config.pubmed,
        &config.xrxiv,
    )
    .await;

    // Merge default sources first (SS + OpenAlex), then append additional source papers if any.
    let mut search_results = merge_search_results(stage_result.ss_results, stage_result.oa_results);
    if !stage_result.additional_results.is_empty() {
        append_additional_results_dedup(&mut search_results, stage_result.additional_results);
    }
    
    let total_papers = search_results.len();
    info!(task_id = %task_id, papers = total_papers, "Search merge completed");

    // Collect source errors for frontend display
    let source_errors: std::collections::HashMap<String, String> = stage_result
        .failed_sources
        .iter()
        .map(|(source, err)| (source.clone(), err.clone()))
        .collect();

    // Log failed sources as warnings (with task_id for traceability)
    if !stage_result.failed_sources.is_empty() {
        for (source, err) in &stage_result.failed_sources {
            warn!(
                task_id = %task_id,
                source = %source,
                error = %err,
                "Search source failed"
            );
        }
    }

    if total_papers == 0 {
        info!(
            task_id = %task_id,
            failed_sources = stage_result.failed_sources.len(),
            "All sources returned 0 papers, pipeline will return empty result"
        );
    }


    // Stage 2: Crossref enrichment (optional, ONLY for papers missing DOI)
    tracker.update("Enriching metadata", 30).await;
    
    let mut enriched_results = search_results;
    if config.enable_crossref && !enriched_results.is_empty() {
        // Find papers that are missing DOI (fallback mode)
        let papers_needing_doi: Vec<(usize, String)> = enriched_results.iter()
            .enumerate()
            .filter(|(_, p)| p.doi.is_empty())
            .map(|(i, p)| (i, p.title.clone()))
            .collect();
        
        let missing_count = papers_needing_doi.len();
        if missing_count > 0 {
            info!(
                total = enriched_results.len(),
                missing_doi = missing_count,
                "Crossref lookup for papers missing DOI"
            );
            
            // Use CrossrefClient for batch lookup by title (only for missing DOIs)
            if let Ok(client) = crossref::CrossrefClient::new(5) {  // Increase concurrency to 5
                let titles: Vec<String> = papers_needing_doi.iter().map(|(_, t)| t.clone()).collect();
                let crossref_results = client.lookup_batch(&titles).await;
                
                // Apply results back to the original papers
                for ((idx, _), cr_opt) in papers_needing_doi.iter().zip(crossref_results.iter()) {
                    if let Some(cr) = cr_opt {
                        let paper = &mut enriched_results[*idx];
                        if paper.doi.is_empty() && !cr.doi.is_empty() {
                            paper.doi = cr.doi.clone();
                        }
                        if paper.abstract_text.is_empty() && !cr.abstract_text.is_empty() {
                            paper.abstract_text = cr.abstract_text.clone();
                        }
                    }
                }
            }
        } else {
            info!(total = enriched_results.len(), "All papers have DOI, skipping Crossref");
        }
    }

    // Stage 3: Semantic Scholar abstracts
    tracker.update("Fetching abstracts", 50).await;
    
    let dois: Vec<String> = enriched_results.iter()
        .filter_map(|p| if p.doi.is_empty() { None } else { Some(p.doi.clone()) })
        .collect();
    
    if !dois.is_empty() {
        match semanticscholar::batch_lookup(&dois, None).await {
            Ok(ss_results) => {
                // SemanticScholarResult: title, doi (String), ss_abstract, oa_pdf_url, ... 
                for paper in &mut enriched_results {
                    if !paper.doi.is_empty() {
                        // Match by DOI (case-insensitive)
                        if let Some(ss) = ss_results.iter().find(|s| {
                            s.doi.eq_ignore_ascii_case(&paper.doi)
                        }) {
                            if paper.abstract_text.is_empty() && !ss.ss_abstract.is_empty() {
                                paper.abstract_text = ss.ss_abstract.clone();
                            }
                            // Populate PDF URL from Semantic Scholar OA PDF
                            if paper.pdf_url.is_empty() && !ss.oa_pdf_url.is_empty() {
                                paper.pdf_url = ss.oa_pdf_url.clone();
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Semantic Scholar lookup failed, continuing");
            }
        }
    }

    // Stage 4: Skip rerank for API (simplified implementation)
    tracker.update("Processing", 70).await;
    
    let mut final_results = enriched_results;

    // Stage 5: EasyScholar rankings with persistent in-process service
    tracker.update("Adding journal rankings", 85).await;
    
    if !config.easyscholar_keys.is_empty() {
        let venues: Vec<String> = final_results
            .iter()
            .filter(|p| !is_preprint_venue(&p.venue))
            .map(|p| p.venue.clone())
            .collect();
        if let Some(service) = ranking_service {
            let request = crate::rankings::RankingBatchRequest {
                task_id: task_id.clone(),
                venues,
            };
            match service.lookup_batch(request).await {
                Ok(result) => {
                    info!(
                        task_id = %task_id,
                        venue_total = result.venue_total,
                        cache_hits = result.cache_hits,
                        cache_misses = result.cache_misses,
                        api_hits = result.api_hits,
                        chunk_size = result.chunk_size_granted,
                        "Ranking service lookup complete"
                    );

                    for paper in final_results.iter_mut() {
                        if paper.venue.is_empty() {
                            continue;
                        }
                        if let Some(ranking) = result.by_venue.get(&paper.venue) {
                            paper.if_score = ranking.sciif.clone();
                            paper.jci_score = ranking.jci.clone();
                            paper.sci_partition = ranking.sci.clone();
                        }
                    }
                }
                Err(e) => {
                    warn!(task_id = %task_id, error = %e, "Ranking service failed, continue without rankings");
                }
            }
        } else {
            warn!(task_id = %task_id, "Ranking keys configured but ranking_service is unavailable");
        }
    }

    // Clone results before filtering (for fallback if filtering yields 0 results)
    let unfiltered_results = final_results.clone();

    // Apply IF/JCI/SCI filters
    if config.sciif.is_some() || config.jci.is_some() || config.sci.is_some() {
        final_results.retain(|p| {
            if is_preprint_venue(&p.venue) {
                // Preprints do not have EasyScholar rankings; never filter them out by ranking criteria.
                return true;
            }
            let mut keep = true;
            if let Some(threshold) = config.sciif {
                if let Some(ref score) = p.if_score {
                    if let Ok(val) = score.parse::<f64>() {
                        keep = keep && val >= threshold;
                    } else {
                        keep = false;
                    }
                } else {
                    keep = false;
                }
            }
            if let Some(threshold) = config.jci {
                if let Some(ref score) = p.jci_score {
                    if let Ok(val) = score.parse::<f64>() {
                        keep = keep && val >= threshold;
                    } else {
                        keep = false;
                    }
                } else {
                    keep = false;
                }
            }
            if let Some(ref pattern) = config.sci {
                if let Some(ref sci) = p.sci_partition {
                    keep = keep && sci.to_lowercase().contains(&pattern.to_lowercase());
                } else {
                    keep = false;
                }
            }
            keep
        });

    }

    // Stage 5: LLM Relevance Filter
    let has_content_help = config
        .content_help
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let llm_filter_applied = llm_filter.is_some() && has_content_help;
    if !has_content_help {
        info!(
            task_id = %task_id,
            papers = final_results.len(),
            "Skipping LLM relevance filtering because content_help is empty"
        );
    } else {
        final_results = apply_llm_relevance_filter(
            tracker,
            &task_id,
            &keyword,
            config.content_help.as_deref(),
            llm_filter.as_ref(),
            final_results,
        )
        .await;
    }

    // Save results to CSV
    tracker.update("Saving results", 95).await;
    
    // If filtered results are empty, use unfiltered results as fallback
    let (results_to_save, is_fallback) = select_results_with_fallback(
        &task_id,
        &config.keyword,
        llm_filter_applied,
        config.llm_strict_filter,
        &final_results,
        &unfiltered_results,
    );
    
    let csv_path = config.output_dir.join("results.csv");
    save_results_csv(&csv_path, &results_to_save)?;

    let filtered_papers = final_results.len();
    log_search_analytics(db, &task_id, &config.keyword, filtered_papers, &final_results).await;
    
    info!(
        task_id = %task_id,
        total = total_papers,
        filtered = filtered_papers,
        saved = results_to_save.len(),
        fallback = is_fallback,
        "Pipeline complete"
    );

    // Compute per-source counts from the final result set (post-merge/dedup/filter).
    // Each paper is attributed to exactly one source; counts sum to filtered_papers.
    let source_counts: std::collections::HashMap<String, usize> = {
        let mut counts = std::collections::HashMap::new();
        for paper in &results_to_save {
            *counts.entry(paper.source.clone()).or_insert(0) += 1;
        }
        counts
    };

    Ok(TaskResult {
        total_papers,
        filtered_papers: results_to_save.len(), // Report actual saved count
        data: serde_json::to_value(&results_to_save).unwrap_or(serde_json::Value::Null),
        csv_path: Some(csv_path.to_string_lossy().to_string()),
        source_counts,
        source_errors,
    })
}

/// Internal paper result structure (for JSON API response)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PaperResult {
    title: String,
    authors: String,
    year: String,
    venue: String,
    doi: String,
    url: String,
    pdf_url: String,
    snippet: String,
    abstract_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    if_score: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jci_score: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sci_partition: Option<String>,
    /// Internal: tracks which search source provided this paper (not serialized to JSON)
    #[serde(skip)]
    source: String,
}

fn append_additional_results_dedup(base: &mut Vec<PaperResult>, additional: Vec<PaperResult>) {
    use std::collections::HashSet;

    let mut doi_set: HashSet<String> = base
        .iter()
        .filter(|p| !p.doi.is_empty())
        .map(|p| p.doi.to_lowercase())
        .collect();
    let mut title_set: HashSet<String> = base
        .iter()
        .map(|p| p.title.to_lowercase())
        .collect();

    for paper in additional {
        let doi_key = if paper.doi.is_empty() {
            None
        } else {
            Some(paper.doi.to_lowercase())
        };
        let title_key = paper.title.to_lowercase();

        let duplicated = doi_key
            .as_ref()
            .map(|d| doi_set.contains(d))
            .unwrap_or(false)
            || title_set.contains(&title_key);
        if duplicated {
            continue;
        }

        if let Some(doi) = doi_key {
            doi_set.insert(doi);
        }
        title_set.insert(title_key);
        base.push(paper);
    }
}

fn is_preprint_venue(venue: &str) -> bool {
    let v = venue.to_lowercase();
    v.contains("arxiv")
        || v.contains("biorxiv")
        || v.contains("medrxiv")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_validation() {
        let valid = PipelineRequest {
            keyword: "machine learning".to_string(),
            ylo: None,
            enable_crossref: Some(true),
            sciif: None,
            jci: None,
            sci: None,
            llm_strict_filter: None,
            content_help: None,
            source_include: None,
            source_exclude: None,
        };
        assert!(valid.validate().is_ok());

        let empty_keyword = PipelineRequest {
            keyword: "".to_string(),
            ..valid.clone()
        };
        assert!(empty_keyword.validate().is_err());
    }

    #[test]
    fn test_pipeline_config_creation() {
        let req = PipelineRequest {
            keyword: "test".to_string(),
            ylo: Some(2020),
            enable_crossref: Some(true),
            sciif: None,
            jci: None,
            sci: None,
            llm_strict_filter: None,
            content_help: None,
            source_include: None,
            source_exclude: None,
        };
        // Config creation should succeed
        let config = PipelineConfig::from_request(
            req,
            &[],
            false,
            false,
            Some(2019),
            true,
            80,
            120,
            &["openalex".to_string(), "semanticscholar".to_string()],
            &SearchArxivSection::default(),
            &SearchPubMedSection::default(),
            &SearchXRxivSection::default(),
        );
        assert!(config.is_ok());
    }

    #[test]
    fn test_paper_result_default() {
        let paper = PaperResult::default();
        assert!(paper.title.is_empty());
        assert!(paper.if_score.is_none());
    }

    #[test]
    fn test_merge_search_results() {
        let ss_paper = PaperResult {
            source: "semanticscholar".to_string(),
            title: "Test Paper".to_string(),
            doi: "10.1234/test".to_string(),
            abstract_text: "SS Abstract".to_string(),
            ..Default::default()
        };
        let oa_paper = PaperResult {
            source: "openalex".to_string(),
            title: "Test Paper".to_string(),
            doi: "10.1234/test".to_string(),
            pdf_url: "https://example.com/paper.pdf".to_string(),
            snippet: "OA Snippet".to_string(),
            ..Default::default()
        };
        
        let merged = merge_search_results(vec![ss_paper], vec![oa_paper]);
        assert_eq!(merged.len(), 1);
        
        let paper = &merged[0];
        assert_eq!(paper.abstract_text, "SS Abstract"); // Prefer SS
        assert_eq!(paper.pdf_url, "https://example.com/paper.pdf"); // Merge from OA
        assert_eq!(paper.snippet, "OA Snippet"); // Merge from OA
    }

    #[test]
    fn test_resolve_enabled_sources_with_include_and_exclude() {
        let server_defaults = vec!["openalex".to_string(), "semanticscholar".to_string()];
        let resolved = resolve_enabled_sources(
            Some(vec!["OpenAlex".to_string(), "PubMed".to_string()]),
            Some(vec!["pubmed".to_string()]),
            &server_defaults,
        );
        assert_eq!(resolved.unwrap(), vec!["openalex".to_string()]);
    }

    #[test]
    fn test_resolve_enabled_sources_falls_back_to_server_defaults() {
        let server_defaults = vec!["openalex".to_string(), "semanticscholar".to_string()];
        let resolved = resolve_enabled_sources(None, None, &server_defaults);
        assert_eq!(resolved.unwrap(), server_defaults);
    }

    #[test]
    fn test_resolve_enabled_sources_rejects_unknown() {
        let server_defaults = vec!["openalex".to_string()];
        let resolved = resolve_enabled_sources(
            Some(vec!["openalex".to_string(), "foo".to_string()]),
            None,
            &server_defaults,
        );
        assert!(resolved.is_err());
    }
}
