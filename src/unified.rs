//! Unified CSV Generation Module
//!
//! Creates the final unified dataset by joining EasyScholar results with Semantic Scholar data.
//! Handles abstract priority (Semantic Scholar > OpenAlex) and date normalization.

use crate::semanticscholar::SemanticScholarResult;
use serde::Serialize;
use std::collections::HashMap;
use tracing::info;

/// Unified result combining all pipeline stages
#[derive(Debug, Serialize)]
pub struct UnifiedResult {
    pub title: String,
    pub author: String,
    pub date: String,
    pub doi: String,
    pub article_url: String,
    pub pdf_url: String,
    pub abstract_text: String,
    pub tldr: String,
    pub journal: String,
    pub if_score: String,
    pub jci_score: String,
    pub sci_partition: String,
}

/// Input item from EasyScholar enriched results
pub struct EnrichedInput {
    pub title: String,
    pub author: String,
    pub year: String,
    pub publication_date: String,
    pub doi: String,
    pub article_url: String,
    pub abstract_text: String,
    pub journal: String,
    pub if_score: String,
    pub jci_score: String,
    pub sci_partition: String,
}

/// CSV column order for unified output
pub const UNIFIED_COLUMNS: &[&str] = &[
    "title", "author", "date", "doi", "article_url", "pdf_url", 
    "abstract_text", "tldr", "journal", "if_score", "jci_score", "sci_partition"
];

/// Generate unified results by joining EasyScholar with Semantic Scholar data.
///
/// # Arguments
/// * `enriched_items` - Items from EasyScholar stage (Stage 3)
/// * `ss_results` - Results from Semantic Scholar batch lookup (Stage 4)
///
/// # Returns
/// Vector of unified results with merged data
pub fn generate_unified(
    enriched_items: &[EnrichedInput],
    ss_results: &[SemanticScholarResult],
) -> Vec<UnifiedResult> {
    info!(
        enriched_items = enriched_items.len(),
        semantic_scholar_items = ss_results.len(),
        "Generating unified output dataset"
    );

    // Build DOI -> Semantic Scholar lookup map
    let ss_map: HashMap<String, &SemanticScholarResult> = ss_results
        .iter()
        .filter(|r| !r.doi.is_empty())
        .map(|r| (r.doi.to_lowercase(), r))
        .collect();

    enriched_items
        .iter()
        .filter(|r| !r.doi.is_empty())
        .map(|r| {
            let doi_lower = r.doi.to_lowercase();
            let ss_data = ss_map.get(&doi_lower);

            // Abstract priority: Semantic Scholar > OpenAlex/Crossref
            let abstract_text = ss_data
                .map(|s| s.ss_abstract.clone())
                .filter(|a| !a.is_empty())
                .unwrap_or_else(|| r.abstract_text.clone());

            // TLDR from Semantic Scholar
            let tldr = ss_data.map(|s| s.tldr.clone()).unwrap_or_default();

            // Article URL: prefer OpenAlex/original, fallback to SS
            let article_url = if !r.article_url.is_empty() {
                r.article_url.clone()
            } else {
                ss_data.map(|s| s.ss_url.clone()).unwrap_or_default()
            };

            // PDF URL: from Semantic Scholar OA PDF
            let pdf_url = ss_data.map(|s| s.oa_pdf_url.clone()).unwrap_or_default();

            // Date: prefer publication_date (YYYY-MM-DD), fallback to year
            let date = if !r.publication_date.is_empty() {
                r.publication_date.clone()
            } else {
                r.year.clone()
            };

            UnifiedResult {
                title: r.title.clone(),
                author: r.author.clone(),
                date,
                doi: r.doi.clone(),
                article_url,
                pdf_url,
                abstract_text,
                tldr,
                journal: r.journal.clone(),
                if_score: r.if_score.clone(),
                jci_score: r.jci_score.clone(),
                sci_partition: r.sci_partition.clone(),
            }
        })
        .collect()
}
