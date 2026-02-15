use crate::llm::{self, LlmRelevanceFilter};
use std::sync::Arc;
use tracing::{debug, info};

use super::{PaperResult, ProgressTracker};

pub(super) async fn apply_llm_relevance_filter(
    tracker: &mut ProgressTracker,
    task_id: &str,
    keyword: &str,
    content_help: Option<&str>,
    llm_filter: Option<&Arc<LlmRelevanceFilter>>,
    mut final_results: Vec<PaperResult>,
) -> Vec<PaperResult> {
    let Some(llm) = llm_filter else {
        return final_results;
    };
    if final_results.is_empty() {
        return final_results;
    }

    tracker.update("LLM Relevance Filtering", 90).await;

    let has_context = content_help.is_some();
    info!(
        task_id = %task_id,
        count = final_results.len(),
        has_context = has_context,
        "Starting LLM filtering"
    );

    let paper_infos: Vec<llm::PaperInfo> = final_results
        .iter()
        .map(|p| llm::PaperInfo {
            title: p.title.clone(),
            abstract_text: p.abstract_text.clone(),
            venue: p.venue.clone(),
        })
        .collect();

    let relevance_results = llm
        .batch_check_relevance(keyword, paper_infos, content_help)
        .await;

    let old_count = final_results.len();
    let mut new_results = Vec::new();

    let mut total_input_chars = 0;
    let mut total_output_chars = 0;

    for (index, res) in relevance_results.iter().enumerate() {
        if res.is_relevant {
            new_results.push(final_results[index].clone());
        } else {
            debug!(
                task_id = %task_id,
                title = %final_results[index].title,
                reason = ?res.reason,
                "Filtered out by LLM"
            );
        }

        if let Some(reason) = &res.reason {
            total_output_chars += reason.len();
        }
        total_input_chars += final_results[index].title.len() + final_results[index].abstract_text.len();
    }

    final_results = new_results;
    info!(
        task_id = %task_id,
        input_papers = old_count,
        output_papers = final_results.len(),
        est_input_tokens = total_input_chars / 4,
        est_output_tokens = total_output_chars / 4,
        "LLM Filtering Complete"
    );

    final_results
}
