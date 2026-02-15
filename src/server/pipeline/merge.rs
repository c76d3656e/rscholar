use std::collections::HashMap;
use tracing::info;

use super::PaperResult;

pub(super) fn merge_search_results(ss_results: Vec<PaperResult>, oa_results: Vec<PaperResult>) -> Vec<PaperResult> {
    let mut doi_map: HashMap<String, PaperResult> = HashMap::new();
    let mut no_doi_results: Vec<PaperResult> = Vec::new();

    for paper in ss_results {
        if paper.doi.is_empty() {
            no_doi_results.push(paper);
        } else {
            let key = paper.doi.to_lowercase();
            doi_map.insert(key, paper);
        }
    }

    for paper in oa_results {
        if paper.doi.is_empty() {
            let title_lower = paper.title.to_lowercase();
            let exists = doi_map.values().any(|p| p.title.to_lowercase() == title_lower)
                || no_doi_results.iter().any(|p| p.title.to_lowercase() == title_lower);
            if !exists {
                no_doi_results.push(paper);
            }
        } else {
            let key = paper.doi.to_lowercase();
            if let Some(existing) = doi_map.get_mut(&key) {
                if existing.abstract_text.is_empty() && !paper.abstract_text.is_empty() {
                    existing.abstract_text = paper.abstract_text;
                }
                if existing.pdf_url.is_empty() && !paper.pdf_url.is_empty() {
                    existing.pdf_url = paper.pdf_url;
                }
                if existing.url.is_empty() && !paper.url.is_empty() {
                    existing.url = paper.url;
                }
                if existing.snippet.is_empty() && !paper.snippet.is_empty() {
                    existing.snippet = paper.snippet;
                }
                if existing.venue.is_empty() && !paper.venue.is_empty() {
                    existing.venue = paper.venue;
                }
            } else {
                doi_map.insert(key, paper);
            }
        }
    }

    let mut results: Vec<PaperResult> = doi_map.into_values().collect();
    results.extend(no_doi_results);

    info!(merged = results.len(), "Results merged and deduplicated");
    results
}
