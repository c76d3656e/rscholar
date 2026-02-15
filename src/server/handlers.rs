//! HTTP endpoint handlers.
//!
//! All API endpoints are defined here.
//! Uses DB-first task creation for persistence and fallback queries for restart recovery.

use super::pipeline::{PipelineConfig, PipelineRequest};
use super::state::AppState;
use super::task::{Task, TaskStatus};
use crate::db::tasks as db_tasks;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use tracing::{error, info, warn};

// ============================================================================
// Response Types
// ============================================================================

/// Standard API error response
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::BAD_REQUEST;
        (status, Json(self)).into_response()
    }
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
}

/// Available sources response
#[derive(Debug, Serialize)]
pub struct SourcesResponse {
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
}

/// Pipeline submission response
#[derive(Debug, Serialize)]
pub struct PipelineResponse {
    pub task_id: String,
    pub status: String,
    pub eta_seconds: u64,
}

/// Task status response
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String,
    pub progress: TaskProgressResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResultResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskProgressResponse {
    pub step: String,
    pub percent: u8,
}

#[derive(Debug, Serialize)]
pub struct TaskResultResponse {
    pub total_papers: usize,
    pub filtered_papers: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csv_path: Option<String>,
    pub data: serde_json::Value,
    /// Per-source paper counts (before merge/dedup)
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub source_counts: std::collections::HashMap<String, usize>,
    /// Per-source error messages (sources that failed)
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub source_errors: std::collections::HashMap<String, String>,
}

// ============================================================================
// Handlers
// ============================================================================

// ============================================================================
// Helper Functions
// ============================================================================

/// Get task from memory cache first, fallback to database.
/// This enables task retrieval after server restart.
async fn get_task_with_fallback(
    state: &AppState,
    task_id: &str,
) -> Result<Task, (StatusCode, Json<ApiError>)> {
    // Try memory cache first (fast path for active tasks)
    if let Some(mem_task) = state.task_store.get(task_id) {
        return Ok(mem_task);
    }

    // Fallback to database (for tasks after server restart)
    let task_id_owned = task_id.to_string();
    match state.run_db(move |conn| db_tasks::get_by_id(conn, &task_id_owned)).await {
        Ok(Some(db_task)) => {
            // Convert DB task to memory task
            let mem_task = Task::from_db_task(&db_task);
            // Optionally cache running tasks in memory for future fast access
            if db_task.status == db_tasks::TaskStatus::Running {
                state.task_store.insert(mem_task.clone());
            }
            Ok(mem_task)
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Task not found".to_string(),
                details: Some(format!("Task ID: {}", task_id)),
            }),
        )),
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "Database lookup failed");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Database error".to_string(),
                    details: Some(e.to_string()),
                }),
            ))
        }
    }
}

/// GET /health - Health check endpoint (no auth required)
pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = state.task_store.uptime();
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime.as_secs(),
    })
}

/// GET /sources - Return enabled search sources from server config
pub async fn sources_handler(State(state): State<AppState>) -> Json<SourcesResponse> {
    fn label_for(id: &str) -> &'static str {
        match id {
            "openalex" => "OpenAlex",
            "semanticscholar" => "Semantic Scholar",
            "arxiv" => "arXiv",
            "pubmed" => "PubMed",
            "biorxiv" => "bioRxiv",
            "medrxiv" => "medRxiv",
            _ => "Unknown",
        }
    }
    let sources = state
        .config
        .search
        .effective_sources()
        .iter()
        .map(|id| SourceInfo {
            id: id.clone(),
            label: label_for(id).to_string(),
        })
        .collect();
    Json(SourcesResponse { sources })
}

/// POST /tasks - Submit a new pipeline job
/// 
/// Uses DB-first pattern: task is persisted to database before returning task_id.
/// This ensures task can be queried even after server restart.
pub async fn pipeline_handler(
    State(state): State<AppState>,
    Json(request): Json<PipelineRequest>,
) -> Result<Json<PipelineResponse>, ApiError> {
    // Validate request
    if let Err(e) = request.validate() {
        return Err(ApiError {
            error: "Validation error".to_string(),
            details: Some(e.to_string()),
        });
    }

    // Create pipeline config first (to get keyword for logging)
    let config = match PipelineConfig::from_request(
        request.clone(),
        &state.config.easyscholar.keys,
        state.config.llm.enable_filter,
        state.config.llm.strict_filter,
        state.config.search.default_ylo,
        state.config.search.enable_crossref,
        state.config.search.ss_limit,
        state.config.search.oa_limit,
        &state.config.search.effective_sources(),
        &state.config.search.arxiv,
        &state.config.search.pubmed,
        &state.config.search.xrxiv,
    ) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create pipeline config");
            return Err(ApiError {
                error: "Configuration error".to_string(),
                details: Some(e.to_string()),
            });
        }
    };

    // DB-first: Create task in database first (atomic check)
    let db_task = db_tasks::Task::new(&config.keyword, "combined");
    let task_id = db_task.id.clone();

    // Write to DB first - if this fails, don't return task_id
    let db_task_clone = db_task.clone();
    if let Err(e) = state.run_db(move |conn| db_tasks::insert(conn, &db_task_clone)).await {
        error!(error = %e, "Failed to persist task to database");
        return Err(ApiError {
            error: "Failed to create task".to_string(),
            details: Some(e.to_string()),
        });
    }

    // Then cache in memory for fast access
    let mem_task = Task::from_db_task(&db_task);
    state.task_store.insert(mem_task);

    info!(
        task_id = %task_id,
        keyword = %config.keyword,
        selected_sources = ?config.enabled_sources,
        "Pipeline job submitted"
    );

    // Spawn background execution with database for caching/analytics
    super::pipeline::spawn_pipeline(
        state.task_store.clone(),
        task_id.clone(),
        config,
        Some(state.db.clone()),
        state.llm_filter.clone(),
        state.ranking_service.clone(),
    );

    Ok(Json(PipelineResponse {
        task_id,
        status: "pending".to_string(),
        eta_seconds: 120,
    }))
}


/// GET /tasks/{id} - Get task status and result
/// 
/// Uses fallback pattern: try memory cache first, then database.
/// This enables task retrieval after server restart.
pub async fn task_status_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<ApiError>)> {
    let task = get_task_with_fallback(&state, &task_id).await?;

    let result = task.result.map(|r| TaskResultResponse {
        total_papers: r.total_papers,
        filtered_papers: r.filtered_papers,
        csv_path: r.csv_path,
        data: r.data,
        source_counts: r.source_counts,
        source_errors: r.source_errors,
    });

    let status = match task.status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
    };

    Ok(Json(TaskResponse {
        task_id: task.id,
        status: status.to_string(),
        progress: TaskProgressResponse {
            step: task.progress.step,
            percent: task.progress.percent,
        },
        result,
        error: task.error,
    }))
}


/// GET /tasks/{id}/download - Download CSV result
/// 
/// Uses fallback pattern for task lookup to support retrieval after server restart.
pub async fn task_download_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let task = get_task_with_fallback(&state, &task_id).await?;

    if task.status != TaskStatus::Completed {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Task not completed".to_string(),
                details: Some(format!("Current status: {:?}", task.status)),
            }),
        ));
    }

    let csv_path = task
        .result
        .and_then(|r| r.csv_path)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "No CSV file available".to_string(),
                    details: None,
                }),
            )
        })?;

    // Read CSV file
    let content = std::fs::read_to_string(&csv_path).map_err(|e| {
        error!(path = %csv_path, error = %e, "Failed to read CSV");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "Failed to read result file".to_string(),
                details: Some(e.to_string()),
            }),
        )
    })?;

    // Return as CSV download
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/csv; charset=utf-8")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}.csv\"", task_id),
        )
        .body(axum::body::Body::from(content))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Response build failed".to_string(),
                    details: Some(e.to_string()),
                }),
            )
        })?;

    Ok(response)
}

/// GET /tasks/:id/bibtex - Download BibTeX bibliography file
/// 
/// Generates a BibTeX file from the task results for use with LaTeX/citation managers.
/// 
/// Uses fallback pattern for task lookup to support retrieval after server restart.
pub async fn task_bibtex_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let task = get_task_with_fallback(&state, &task_id).await?;

    if task.status != TaskStatus::Completed {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Task not completed".to_string(),
                details: Some(format!("Current status: {:?}", task.status)),
            }),
        ));
    }

    // Extract papers from task result
    let result = task.result.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "No results available".to_string(),
                details: None,
            }),
        )
    })?;

    // Parse the data JSON into papers
    let papers: Vec<BibTexPaper> = match serde_json::from_value(result.data.clone()) {
        Ok(value) => value,
        Err(error) => {
            warn!(
                task_id = %task_id,
                error = %error,
                payload_preview = %result.data.to_string().chars().take(200).collect::<String>(),
                "Failed to parse task result data as BibTeX papers"
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Failed to parse task result".to_string(),
                    details: Some(error.to_string()),
                }),
            ));
        }
    };
    
    if papers.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "No papers in result".to_string(),
                details: None,
            }),
        ));
    }

    // Generate BibTeX content
    let bibtex_content = generate_bibtex(&papers, &task_id);

    info!(
        task_id = %task_id,
        papers = papers.len(),
        "BibTeX file generated"
    );

    // Return as BibTeX download
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-bibtex; charset=utf-8")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}.bib\"", task_id),
        )
        .body(axum::body::Body::from(bibtex_content))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Response build failed".to_string(),
                    details: Some(e.to_string()),
                }),
            )
        })?;

    Ok(response)
}

// ============================================================================
// BibTeX Generation
// ============================================================================

/// Paper structure for BibTeX generation
#[derive(Debug, serde::Deserialize)]
struct BibTexPaper {
    title: String,
    authors: String,
    year: String,
    venue: String,
    doi: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    abstract_text: String,
}

/// Generate BibTeX content from a list of papers
/// 
/// Each paper is formatted as a BibTeX @article entry with:
/// - Citation key: `{first_author_lastname}{year}_{index}`
/// - Required fields: title, author, year, journal
/// - Optional fields: doi, url, abstract
fn generate_bibtex(papers: &[BibTexPaper], task_id: &str) -> String {
    let mut entries = Vec::new();
    
    // Add header comment
    entries.push(format!(
        "% BibTeX bibliography generated by Rscholar\n% Task ID: {}\n% Generated: {}\n% Total entries: {}\n",
        task_id,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        papers.len()
    ));

    for (idx, paper) in papers.iter().enumerate() {
        let citation_key = generate_citation_key(paper, idx);
        let entry = format_bibtex_entry(paper, &citation_key);
        entries.push(entry);
    }

    entries.join("\n")
}

/// Generate a unique citation key for a paper
/// Format: {FirstAuthorLastName}{Year}_{Index}
/// Example: Smith2024_001
fn generate_citation_key(paper: &BibTexPaper, index: usize) -> String {
    // Extract first author's last name
    let first_author = paper.authors.split(&[',', ';', '&'][..])
        .next()
        .unwrap_or("Unknown")
        .trim();
    
    // Get last name (assume format "First Last" or "Last, First")
    let last_name = if first_author.contains(',') {
        // Format: "Last, First"
        first_author.split(',').next().unwrap_or(first_author).trim()
    } else {
        // Format: "First Last" - get last word
        first_author.split_whitespace().last().unwrap_or(first_author)
    };
    
    // Clean the last name (remove non-alphanumeric)
    let clean_name: String = last_name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    
    let year = if paper.year.is_empty() { "NoYear" } else { &paper.year };
    
    format!("{}{}_{:03}", clean_name, year, index + 1)
}

/// Format a single BibTeX entry
fn format_bibtex_entry(paper: &BibTexPaper, citation_key: &str) -> String {
    let mut entry = format!("@article{{{},\n", citation_key);
    
    // Title (required)
    entry.push_str(&format!("  title = {{{}}},\n", escape_bibtex(&paper.title)));
    
    // Author (required) - convert to BibTeX format "Last, First and Last2, First2"
    let authors = format_authors_bibtex(&paper.authors);
    entry.push_str(&format!("  author = {{{}}},\n", authors));
    
    // Year (required)
    if !paper.year.is_empty() {
        entry.push_str(&format!("  year = {{{}}},\n", paper.year));
    }
    
    // Journal/Venue
    if !paper.venue.is_empty() {
        entry.push_str(&format!("  journal = {{{}}},\n", escape_bibtex(&paper.venue)));
    }
    
    // DOI
    if !paper.doi.is_empty() {
        entry.push_str(&format!("  doi = {{{}}},\n", paper.doi));
    }
    
    // URL
    if !paper.url.is_empty() {
        entry.push_str(&format!("  url = {{{}}},\n", paper.url));
    }
    
    // Abstract
    if !paper.abstract_text.is_empty() {
        entry.push_str(&format!("  abstract = {{{}}},\n", escape_bibtex(&paper.abstract_text)));
    }
    
    entry.push_str("}\n");
    entry
}

/// Escape special BibTeX characters
fn escape_bibtex(text: &str) -> String {
    text.replace('&', r"\&")
        .replace('%', r"\%")
        .replace('$', r"\$")
        .replace('#', r"\#")
        .replace('_', r"\_")
        .replace('{', r"\{")
        .replace('}', r"\}")
        .replace('~', r"\textasciitilde{}")
        .replace('^', r"\textasciicircum{}")
}

/// Format authors for BibTeX (convert various formats to "Last, First and Last2, First2")
fn format_authors_bibtex(authors: &str) -> String {
    // Split by common separators
    let author_list: Vec<&str> = authors
        .split(&[',', ';'][..])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    
    // If only one author with no comma, just return as-is
    if author_list.len() == 1 && !authors.contains(',') {
        return escape_bibtex(authors.trim());
    }
    
    // Join with " and " for BibTeX
    author_list
        .iter()
        .map(|a| escape_bibtex(a))
        .collect::<Vec<_>>()
        .join(" and ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_response() {
        let error = ApiError {
            error: "Test error".to_string(),
            details: Some("Details".to_string()),
        };
        let json = serde_json::to_string(&error);
        assert!(json.is_ok());
    }

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            version: "0.1.0".to_string(),
            uptime_secs: 3600,
        };
        let json = serde_json::to_string(&response);
        assert!(json.is_ok());
        assert!(json.expect("valid json").contains("healthy"));
    }

    #[test]
    fn test_pipeline_response_serialization() {
        let response = PipelineResponse {
            task_id: "abc-123".to_string(),
            status: "pending".to_string(),
            eta_seconds: 120,
        };
        let json = serde_json::to_string(&response);
        assert!(json.is_ok());
    }
}
