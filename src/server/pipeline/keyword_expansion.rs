use crate::llm::{self, LlmRelevanceFilter};
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::{PipelineConfig, ProgressTracker};

pub(super) async fn run_keyword_expansion(
    tracker: &mut ProgressTracker,
    config: &PipelineConfig,
    llm_filter: Option<&Arc<LlmRelevanceFilter>>,
    keyword_for_expansion: &str,
) -> Vec<String> {
    let Some(content_help) = config.content_help.as_ref() else {
        debug!("No content_help provided, skipping keyword expansion");
        return vec![];
    };
    let Some(llm) = llm_filter else {
        debug!("LLM filter not enabled, skipping keyword expansion");
        return vec![];
    };
    if llm.providers.is_empty() {
        debug!("No LLM provider available for keyword expansion");
        return vec![];
    }

    tracker.update("Expanding keywords (LLM)", 5).await;

    let expansion_request = llm::keyword_expansion::KeywordExpansionRequest {
        keyword: keyword_for_expansion.to_string(),
        descript: content_help.clone(),
    };

    let mut expanded: Option<Vec<String>> = None;
    for provider in &llm.providers {
        debug!(
            provider = %provider.name(),
            keyword = %keyword_for_expansion,
            "Trying keyword expansion provider"
        );
        match llm::keyword_expansion::expand_keywords(provider.as_ref(), &expansion_request).await {
            Ok(result) => {
                info!(
                    provider = %provider.name(),
                    keyword = %keyword_for_expansion,
                    expanded_count = result.extended_keywords.len(),
                    expanded = ?result.extended_keywords,
                    "Keyword expansion successful"
                );
                expanded = Some(result.extended_keywords);
                break;
            }
            Err(error) => {
                warn!(
                    provider = %provider.name(),
                    keyword = %keyword_for_expansion,
                    error = %error,
                    "Keyword expansion provider failed, trying fallback"
                );
            }
        }
    }

    match expanded {
        Some(v) => v,
        None => {
            warn!(
                keyword = %keyword_for_expansion,
                "All keyword expansion providers failed, using original keyword only"
            );
            vec![]
        }
    }
}
