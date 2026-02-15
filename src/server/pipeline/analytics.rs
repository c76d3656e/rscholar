use crate::db::DbPool;
use tracing::{debug, warn};

use super::PaperResult;

pub(super) async fn log_search_analytics(
    db: Option<&DbPool>,
    task_id: &str,
    keyword: &str,
    filtered_papers: usize,
    final_results: &[PaperResult],
) {
    let Some(db_pool) = db else {
        return;
    };

    let journals: Vec<String> = final_results
        .iter()
        .map(|p| p.venue.clone())
        .filter(|v| !v.is_empty())
        .collect();
    let keyword = keyword.to_string();
    let source = "combined".to_string();

    debug!(
        task_id = %task_id,
        keyword = %keyword,
        source = %source,
        filtered_papers = filtered_papers,
        journals = journals.len(),
        api_key_id = "none",
        "Logging analytics search event (API key id not wired yet)"
    );

    match db_pool.get().await {
        Ok(conn) => match conn
            .interact(move |conn| {
                crate::db::analytics::log_search(
                    conn,
                    None,
                    &keyword,
                    Some(&source),
                    filtered_papers as i32,
                    &journals,
                )
            })
            .await
        {
            Ok(Ok(search_id)) => {
                debug!(
                    task_id = %task_id,
                    search_id = search_id,
                    "Analytics search event persisted"
                );
            }
            Ok(Err(error)) => {
                warn!(
                    task_id = %task_id,
                    error = %error,
                    "Failed to persist analytics search event"
                );
            }
            Err(error) => {
                warn!(
                    task_id = %task_id,
                    error = %error,
                    "Failed to run analytics DB interaction"
                );
            }
        },
        Err(error) => {
            warn!(
                task_id = %task_id,
                error = %error,
                "Failed to get DB connection for analytics logging"
            );
        }
    }
}
