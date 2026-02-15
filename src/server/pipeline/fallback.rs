use tracing::warn;

use super::PaperResult;

pub(super) fn select_results_with_fallback(
    task_id: &str,
    keyword: &str,
    llm_filter_enabled: bool,
    llm_strict_filter: bool,
    final_results: &[PaperResult],
    unfiltered_results: &[PaperResult],
) -> (Vec<PaperResult>, bool) {
    if final_results.is_empty() && !unfiltered_results.is_empty() && !llm_strict_filter {
        warn!(
            task_id = %task_id,
            unfiltered = unfiltered_results.len(),
            filtered = final_results.len(),
            keyword = %keyword,
            llm_filter_enabled = llm_filter_enabled,
            llm_strict_filter = llm_strict_filter,
            "Filtered results empty, falling back to unfiltered results"
        );
        return (unfiltered_results.to_vec(), true);
    }

    if final_results.is_empty() && !unfiltered_results.is_empty() && llm_strict_filter {
        warn!(
            task_id = %task_id,
            unfiltered = unfiltered_results.len(),
            filtered = final_results.len(),
            keyword = %keyword,
            llm_strict_filter = llm_strict_filter,
            "Filtered results empty and strict mode enabled; fallback disabled"
        );
    }
    (final_results.to_vec(), false)
}
