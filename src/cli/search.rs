//! Search pipeline CLI handler.
//!
//! Implements the full search and enrichment pipeline:
//! 1. Search (OpenAlex)
//! 2. Crossref/Semantic Scholar enrichment
//! 3. Rerank filtering
//! 4. EasyScholar ranking
//! 5. CSV output
//! 6. Analytics logging (to DB)

use super::SearchArgs;
use anyhow::{Context, Result};
use chrono::Local;
use rscholar::{
    db, openalex, semanticscholar, traffic, unified,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use rusqlite::Connection;
use tracing::{info, warn};

/// Run the full search pipeline
pub async fn run_search_pipeline(args: SearchArgs) -> Result<()> {
    info!(
        keyword = %args.keyword,
        source = %args.source,
        ylo = ?args.ylo,
        has_easyscholar_key = args.easyscholar_key.is_some(),
        "Starting CLI search pipeline"
    );

    // Parse pages
    let pages = parse_pages(&args.pages).context("Invalid --pages format")?;

    // VALIDATION: Check if EasyScholar key is provided when filters are used
    let filter_active = args.sciif.is_some()
        || args.jci.is_some()
        || args.sci.is_some()
        || args.sci_up_top.is_some()
        || args.sci_base.is_some()
        || args.sci_up.is_some();

    if filter_active && args.easyscholar_key.is_none() {
        warn!("CLI search aborted: filters require --easyscholar-key");
        anyhow::bail!("EasyScholar filters (sciif, jci, sci, etc.) require --easyscholar-key provided.");
    }

    // Calculate year filter (default: current year - 5)
    let ylo_val = args.ylo.unwrap_or_else(|| {
        Local::now()
            .format("%Y")
            .to_string()
            .parse()
            .unwrap_or(2020)
            - 5
    });

    // Create output folder
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let safe_keyword: String = args
        .keyword
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>()
        .trim()
        .replace(' ', "_");
    let output_folder = args.output.join(format!("{}_{}", timestamp, safe_keyword));
    std::fs::create_dir_all(&output_folder).context("Failed to create output directory")?;

    println!("Output folder: {}", output_folder.display());
    info!(output = %output_folder.display(), "Created pipeline output directory");

    // Reset traffic stats
    traffic::GLOBAL_TRAFFIC.reset();

    // ===========================================
    // STAGE 1 & 2: Search & Enrichment
    // ===========================================
    let mut enriched_list: Vec<EnrichedResult>;

    if args.source == "openalex" {
        enriched_list = run_openalex_pipeline(&args.keyword, &pages, ylo_val, &output_folder).await?;
    } else {
        warn!(source = %args.source, "CLI search received unsupported source");
        anyhow::bail!("Invalid source: {}. Supported source: openalex", args.source);
    }

    // ===========================================
    // STAGE 2: Semantic Scholar Enrichment
    // ===========================================
    println!("\n--- Stage 2: Semantic Scholar Lookup ---");

    let dois: Vec<String> = enriched_list
        .iter()
        .map(|r| r.doi.clone())
        .filter(|d| !d.is_empty())
        .collect();

    let ss_results = if dois.is_empty() {
        println!("No DOIs found, skipping Semantic Scholar.");
        Vec::new()
    } else {
        println!("Looking up {} papers by DOI...", dois.len());
        match semanticscholar::batch_lookup(&dois, None).await {
            Ok(results) => {
                println!("Found {} papers in Semantic Scholar.", results.len());
                let ss_path = output_folder.join("2b_semanticscholar.csv");
                save_csv(&ss_path, &results, &["doi", "title", "ss_abstract", "tldr"])?;
                results
            }
            Err(e) => {
                println!("Semantic Scholar lookup failed: {}. Continuing.", e);
                Vec::new()
            }
        }
    };

    // Build SS lookup map
    let ss_map: std::collections::HashMap<String, &semanticscholar::SemanticScholarResult> =
        ss_results
            .iter()
            .filter(|r| !r.doi.is_empty())
            .map(|r| (r.doi.to_lowercase(), r))
            .collect();

    // Merge SS data
    for item in &mut enriched_list {
        if let Some(ss) = ss_map.get(&item.doi.to_lowercase()) {
            if !ss.ss_abstract.is_empty() {
                item.abstract_text = ss.ss_abstract.clone();
            }
            if !ss.tldr.is_empty() {
                item.snippet = format!("[TLDR] {}", ss.tldr);
            }
        }
    }

    // Deduplicate
    let original_count = enriched_list.len();
    let mut seen_titles: std::collections::HashSet<String> = std::collections::HashSet::new();
    enriched_list.retain(|item| {
        let normalized = item.title.trim().to_lowercase();
        if normalized.is_empty() {
            true
        } else if seen_titles.contains(&normalized) {
            false
        } else {
            seen_titles.insert(normalized);
            true
        }
    });
    let dedup_count = original_count - enriched_list.len();
    if dedup_count > 0 {
        println!("Deduplicated: removed {} duplicate papers", dedup_count);
    }

    // Save merged
    let merged_path = output_folder.join("2_merged.csv");
    save_csv(&merged_path, &enriched_list, &["title", "doi", "abstract_text"])?;
    println!("Merged {} papers", enriched_list.len());

    // ===========================================
    // STAGE 3: Rerank (Feature Removed)
    // ===========================================
    let reranked_list = enriched_list.clone();

    // ===========================================
    // STAGE 4: EasyScholar Ranking
    // ===========================================
    if let Some(ref keys) = args.easyscholar_key {
        println!("\n--- Stage 4: EasyScholar Ranking ---");
        println!("Processing {} papers", reranked_list.len());

        let ranking_pool = rscholar::rankings::RankingClientPool::new(&keys)?;
        println!(
            "Using {} EasyScholar API keys",
            ranking_pool.key_count()
        );

        let filter_active = args.sciif.is_some()
            || args.jci.is_some()
            || args.sci.is_some()
            || args.sci_up_top.is_some()
            || args.sci_base.is_some()
            || args.sci_up.is_some();

        // Collect unique journals
        let unique_journals: Vec<String> = reranked_list
            .iter()
            .map(|item| item.journal.trim().to_string())
            .filter(|j| !j.is_empty())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        println!("Found {} unique journals to query", unique_journals.len());
        info!(unique_journals = unique_journals.len(), "Prepared journal list for ranking stage");

        // Cache + API Strategy
        let mut journal_rankings: std::collections::HashMap<
            String,
            Option<rscholar::rankings::RankingMetrics>,
        > = std::collections::HashMap::new();
        
        let mut to_fetch = Vec::new();
        let db_path = std::env::var("DATABASE_URL").unwrap_or_else(|_| "data/rscholar.db".to_string());
        
        // 1. Check SQLite Cache
        let mut cache_hits = 0;
        match Connection::open(&db_path) {
            Ok(conn) => {
                for journal in &unique_journals {
                    // Try to get from cache
                    let cached = db::journal_cache::get(&conn, journal).ok().flatten();
                    
                    if let Some(c) = cached {
                        let metrics = rscholar::rankings::RankingMetrics {
                            sciif: c.sciif,
                            jci: c.jci,
                            sci: c.sci,
                            sci_up_top: c.sci_up_top,
                            sci_base: c.sci_base,
                            sci_up: c.sci_up,
                        };
                        journal_rankings.insert(journal.clone(), Some(metrics));
                        cache_hits += 1;
                    } else {
                        to_fetch.push(journal.clone());
                    }
                }
            }
            Err(e) => {
                println!("Warning: Could not open DB for cache: {}. Fetching all.", e);
                to_fetch = unique_journals.clone();
            }
        }
        
        println!("Cache hit: {}. Need to fetch: {}.", cache_hits, to_fetch.len());
        info!(cache_hits, to_fetch = to_fetch.len(), "Ranking cache check complete");

        // 2. Fetch missing from API
        if !to_fetch.is_empty() {
            let api_results = ranking_pool.batch_lookup(&to_fetch).await;
            
            // 3. Save to DB and Update Map
            let conn_opt = Connection::open(&db_path).ok();
            
            for (journal, res_opt) in to_fetch.iter().zip(api_results.into_iter()) {
                journal_rankings.insert(journal.clone(), res_opt.clone());
                
                if let Some(metrics) = res_opt {
                    if let Some(ref conn) = conn_opt {
                         let entry = db::journal_cache::JournalRanking {
                            name: journal.clone(),
                            sciif: metrics.sciif.clone(),
                            jci: metrics.jci.clone(),
                            sci: metrics.sci.clone(),
                            sci_up_top: metrics.sci_up_top.clone(),
                            sci_base: metrics.sci_base.clone(),
                            sci_up: metrics.sci_up.clone(),
                            fetched_at: chrono::Utc::now().timestamp(),
                        };
                        let _ = db::journal_cache::upsert(conn, &entry);
                    }
                }
            }
        }
        
        println!("Completed querying journals.");
        info!("Ranking stage completed for CLI pipeline");

        // Assign rankings
        let mut all_with_rankings: Vec<EnrichedResult> = Vec::new();
        for mut item in reranked_list {
            let journal = item.journal.trim().to_string();
            if !journal.is_empty() {
                if let Some(Some(metrics)) = journal_rankings.get(&journal).cloned() {
                    item.if_score = metrics.sciif.unwrap_or_default();
                    item.jci_score = metrics.jci.unwrap_or_default();
                    item.sci_partition = metrics.sci.unwrap_or_default();
                    item.sci_up_top = metrics.sci_up_top.unwrap_or_default();
                    item.sci_base = metrics.sci_base.unwrap_or_default();
                    item.sci_up = metrics.sci_up.unwrap_or_default();
                }
            }
            all_with_rankings.push(item);
        }

        println!("Enriched: {} results with ranking data", all_with_rankings.len());

        let es_path = output_folder.join("4_easyscholar.csv");
        save_csv(&es_path, &all_with_rankings, &["title", "if_score", "sci_partition"])?;

        // Apply filters
        let filtered_list: Vec<EnrichedResult> = if filter_active {
            all_with_rankings
                .into_iter()
                .filter(|item| passes_filters(item, &args))
                .collect()
        } else {
            all_with_rankings
        };

        if filter_active {
            println!("Filtered: {} results", filtered_list.len());
            let filtered_path = output_folder.join("5_easyscholar_filtered.csv");
            save_csv(&filtered_path, &filtered_list, &["title", "if_score", "sci_partition"])?;
        }

        // Create final output
        create_final_output(&filtered_list, &ss_results, &output_folder)?;
        
        // Log analytics to DB
        log_search_analytics(
            &args.keyword,
            &args.source,
            filtered_list.len(),
            &filtered_list.iter().map(|r| r.journal.clone()).collect::<Vec<_>>(),
        );
    } else {
        println!("\n--- Stage 4: Skipped (no --easyscholar-key provided) ---");
        create_final_output(&reranked_list, &ss_results, &output_folder)?;
        
        // Log analytics to DB
        log_search_analytics(
            &args.keyword,
            &args.source,
            reranked_list.len(),
            &reranked_list.iter().map(|r| r.journal.clone()).collect::<Vec<_>>(),
        );
    }

    // Print Traffic Stats
    let traffic_stats = traffic::GLOBAL_TRAFFIC.snapshot();
    println!("\n--- Traffic Summary ---");
    println!("{}", traffic_stats.to_string_mb());

    println!(
        "\n✓ Pipeline complete. Results in: {}",
        output_folder.display()
    );
    info!(output = %output_folder.display(), "CLI search pipeline completed");
    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Run OpenAlex pipeline
async fn run_openalex_pipeline(
    keyword: &str,
    pages: &[i32],
    ylo: i32,
    output_folder: &PathBuf,
) -> Result<Vec<EnrichedResult>> {
    println!("\n--- Stage 1: OpenAlex Search ---");

    let query_options = openalex::QueryOptions {
        pages: pages.to_vec(),
        ylo: Some(ylo),
        yhi: None,
        all_results: true,
    };

    let oa_results = openalex::query(keyword, &query_options).await?;

    if oa_results.is_empty() {
        println!("No results from OpenAlex.");
        return Ok(Vec::new());
    }

    println!("Found {} results from OpenAlex.", oa_results.len());

    let oa_path = output_folder.join("1_openalex.csv");
    save_csv(&oa_path, &oa_results, &["title", "author", "year", "doi"])?;

    let enriched_list: Vec<EnrichedResult> = oa_results
        .into_iter()
        .map(|oa| EnrichedResult {
            title: oa.title,
            author: oa.author,
            year: oa.year,
            publication_date: oa.publication_date,
            venue: oa.venue.clone(),
            article_url: oa.article_url,
            citations: oa.citations,
            snippet: oa.snippet.clone(),
            doi: oa.doi,
            journal: oa.venue,
            crossref_authors: String::new(),
            crossref_date: String::new(),
            abstract_text: oa.snippet,
            if_score: String::new(),
            jci_score: String::new(),
            sci_partition: String::new(),
            sci_up_top: String::new(),
            sci_base: String::new(),
            sci_up: String::new(),
        })
        .collect();

    Ok(enriched_list)
}

/// Check if item passes all filters
fn passes_filters(item: &EnrichedResult, args: &SearchArgs) -> bool {
    if let Some(threshold) = args.sciif {
        let if_val: f64 = item.if_score.parse().unwrap_or(0.0);
        if if_val < threshold {
            return false;
        }
    }

    if let Some(threshold) = args.jci {
        let jci_val: f64 = item.jci_score.parse().unwrap_or(0.0);
        if jci_val < threshold {
            return false;
        }
    }

    if let Some(ref pattern) = args.sci {
        if !item.sci_partition.to_lowercase().contains(&pattern.to_lowercase()) {
            return false;
        }
    }

    if let Some(ref pattern) = args.sci_up_top {
        if !item.sci_up_top.to_lowercase().contains(&pattern.to_lowercase()) {
            return false;
        }
    }

    if let Some(ref pattern) = args.sci_base {
        if !item.sci_base.to_lowercase().contains(&pattern.to_lowercase()) {
            return false;
        }
    }

    if let Some(ref pattern) = args.sci_up {
        if !item.sci_up.to_lowercase().contains(&pattern.to_lowercase()) {
            return false;
        }
    }

    true
}

/// Create final unified output
fn create_final_output(
    results: &[EnrichedResult],
    ss_results: &[semanticscholar::SemanticScholarResult],
    output_folder: &PathBuf,
) -> Result<()> {
    println!("\n--- Creating Final Output ---");

    let enriched_inputs: Vec<unified::EnrichedInput> = results
        .iter()
        .map(|r| unified::EnrichedInput {
            title: r.title.clone(),
            author: r.author.clone(),
            year: r.year.clone(),
            publication_date: r.publication_date.clone(),
            doi: r.doi.clone(),
            article_url: r.article_url.clone(),
            abstract_text: r.abstract_text.clone(),
            journal: r.journal.clone(),
            if_score: r.if_score.clone(),
            jci_score: r.jci_score.clone(),
            sci_partition: r.sci_partition.clone(),
        })
        .collect();

    let unified_results = unified::generate_unified(&enriched_inputs, ss_results);

    let unified_path = output_folder.join("5_final.csv");
    save_csv(&unified_path, &unified_results, unified::UNIFIED_COLUMNS)?;
    println!("Created final dataset: {} papers", unified_results.len());

    Ok(())
}

/// Parse page range string (e.g., "1", "1-10")
fn parse_pages(pages_str: &str) -> Result<Vec<i32>> {
    if pages_str.contains('-') {
        let parts: Vec<&str> = pages_str.split('-').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid page range format");
        }
        let start: i32 = parts[0].parse().context("Invalid start page")?;
        let end: i32 = parts[1].parse().context("Invalid end page")?;
        Ok((start..=end).collect())
    } else {
        let page: i32 = pages_str.parse().context("Invalid page number")?;
        Ok(vec![page])
    }
}

/// Enriched result used by the CLI pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedResult {
    pub title: String,
    pub author: String,
    pub year: String,
    pub publication_date: String,
    pub venue: String,
    pub article_url: String,
    pub citations: String,
    pub snippet: String,
    pub doi: String,
    pub journal: String,
    pub crossref_authors: String,
    pub crossref_date: String,
    pub abstract_text: String,
    pub if_score: String,
    pub jci_score: String,
    pub sci_partition: String,
    pub sci_up_top: String,
    pub sci_base: String,
    pub sci_up: String,
}

/// Save data to CSV file
fn save_csv<T: Serialize>(
    path: &std::path::Path,
    data: &[T],
    _priority_fields: &[&str],
) -> Result<()> {
    if data.is_empty() {
        println!("No data to save to {:?}", path);
        return Ok(());
    }

    let mut wtr = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(path)
        .context("Failed to create CSV writer")?;

    for item in data {
        wtr.serialize(item).context("Failed to write CSV record")?;
    }

    wtr.flush().context("Failed to flush CSV")?;
    println!("Saved: {:?}", path);
    Ok(())
}

/// Log search analytics to SQLite database
fn log_search_analytics(keyword: &str, source: &str, result_count: usize, journals: &[String]) {
    // Initialize DB (best effort, don't fail pipeline if DB unavailable)
    let db_config = db::DbConfig::default();
    
    match db::init_pool(&db_config) {
        Ok(pool) => {
            // Get sync connection for logging
            let conn = match rusqlite::Connection::open(&db_config.path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Warning: Could not open DB for analytics: {}", e);
                    return;
                }
            };
            
            // Filter non-empty journals
            let unique_journals: Vec<String> = journals
                .iter()
                .filter(|j| !j.is_empty())
                .cloned()
                .collect();
            
            match db::analytics::log_search(
                &conn,
                None, // No API key in CLI mode
                keyword,
                Some(source),
                result_count as i32,
                &unique_journals,
            ) {
                Ok(search_id) => {
                    println!("📊 Analytics logged (search_id: {})", search_id);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to log analytics: {}", e);
                }
            }
            
            // Drop pool to ensure clean shutdown
            drop(pool);
        }
        Err(e) => {
            eprintln!("Warning: Could not initialize DB for analytics: {}", e);
        }
    }
}
