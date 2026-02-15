//! Task persistence layer.
//!
//! CRUD operations for tasks stored in SQLite.

use crate::error::{GscholarError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Task status enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TaskStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Task progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub step: String,
    pub percent: u8,
}

impl Default for TaskProgress {
    fn default() -> Self {
        Self {
            step: "Initializing".to_string(),
            percent: 0,
        }
    }
}

/// Task result data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub total_papers: usize,
    pub filtered_papers: usize,
    pub data: serde_json::Value,
    pub csv_path: Option<String>,
}

/// Task entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub status: TaskStatus,
    pub progress: TaskProgress,
    pub result: Option<TaskResult>,
    pub error: Option<String>,
    pub keyword: Option<String>,
    pub source: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Task {
    /// Create a new pending task
    pub fn new(keyword: &str, source: &str) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            status: TaskStatus::Pending,
            progress: TaskProgress::default(),
            result: None,
            error: None,
            keyword: Some(keyword.to_string()),
            source: Some(source.to_string()),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Insert a new task
pub fn insert(conn: &Connection, task: &Task) -> Result<()> {
    let result_json = task.result.as_ref()
        .map(|r| serde_json::to_string(r).unwrap_or_default());

    conn.execute(
        "INSERT INTO tasks (id, status, progress_step, progress_percent, result_json, error, keyword, source, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            task.id,
            task.status.as_str(),
            task.progress.step,
            task.progress.percent,
            result_json,
            task.error,
            task.keyword,
            task.source,
            task.created_at,
            task.updated_at,
        ],
    ).map_err(|e| GscholarError::Database(format!("Insert task failed: {}", e)))?;

    debug!(task_id = %task.id, "Task inserted");
    Ok(())
}

/// Get a task by ID
pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Task>> {
    let task = conn.query_row(
        "SELECT id, status, progress_step, progress_percent, result_json, error, keyword, source, created_at, updated_at
         FROM tasks WHERE id = ?1",
        params![id],
        |row| {
            let result_json: Option<String> = row.get(4)?;
            let result = result_json.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Task {
                id: row.get(0)?,
                status: TaskStatus::from_str(&row.get::<_, String>(1)?),
                progress: TaskProgress {
                    step: row.get(2)?,
                    percent: row.get::<_, i32>(3)? as u8,
                },
                result,
                error: row.get(5)?,
                keyword: row.get(6)?,
                source: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        },
    ).optional()
    .map_err(|e| GscholarError::Database(format!("Get task failed: {}", e)))?;

    Ok(task)
}

/// Update task progress
pub fn update_progress(conn: &Connection, id: &str, step: &str, percent: u8) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let status = if percent > 0 { "running" } else { "pending" };

    conn.execute(
        "UPDATE tasks SET status = ?1, progress_step = ?2, progress_percent = ?3, updated_at = ?4 WHERE id = ?5",
        params![status, step, percent as i32, now, id],
    ).map_err(|e| GscholarError::Database(format!("Update progress failed: {}", e)))?;

    Ok(())
}

/// Complete a task
pub fn complete(conn: &Connection, id: &str, result: &TaskResult) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let result_json = serde_json::to_string(result).unwrap_or_default();

    conn.execute(
        "UPDATE tasks SET status = 'completed', progress_step = 'Completed', progress_percent = 100, result_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![result_json, now, id],
    ).map_err(|e| GscholarError::Database(format!("Complete task failed: {}", e)))?;

    Ok(())
}

/// Fail a task
pub fn fail(conn: &Connection, id: &str, error: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "UPDATE tasks SET status = 'failed', progress_step = 'Failed', error = ?1, updated_at = ?2 WHERE id = ?3",
        params![error, now, id],
    ).map_err(|e| GscholarError::Database(format!("Fail task failed: {}", e)))?;

    Ok(())
}

/// Delete a task
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
        .map_err(|e| GscholarError::Database(format!("Delete task failed: {}", e)))?;
    
    Ok(rows > 0)
}

/// List tasks with pagination
pub fn list(conn: &Connection, page: u32, limit: u32, status_filter: Option<&str>) -> Result<(Vec<Task>, i64)> {
    let offset = (page.saturating_sub(1)) * limit;

    // Get total count
    let total: i64 = if let Some(status) = status_filter {
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = ?1",
            params![status],
            |row| row.get(0),
        ).unwrap_or(0)
    } else {
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0)).unwrap_or(0)
    };

    // Query with pagination
    let sql = if status_filter.is_some() {
        "SELECT id, status, progress_step, progress_percent, result_json, error, keyword, source, created_at, updated_at
         FROM tasks WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
    } else {
        "SELECT id, status, progress_step, progress_percent, result_json, error, keyword, source, created_at, updated_at
         FROM tasks ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
    };

    let mut stmt = conn.prepare(sql)
        .map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = if let Some(status) = status_filter {
        stmt.query_map(params![status, limit, offset], row_to_task)
    } else {
        stmt.query_map(params![limit, offset], row_to_task)
    }.map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let tasks: Vec<Task> = rows.filter_map(|r| r.ok()).collect();
    Ok((tasks, total))
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let result_json: Option<String> = row.get(4)?;
    let result = result_json.and_then(|s| serde_json::from_str(&s).ok());

    Ok(Task {
        id: row.get(0)?,
        status: TaskStatus::from_str(&row.get::<_, String>(1)?),
        progress: TaskProgress {
            step: row.get(2)?,
            percent: row.get::<_, i32>(3)? as u8,
        },
        result,
        error: row.get(5)?,
        keyword: row.get(6)?,
        source: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

/// Cleanup old tasks
pub fn cleanup(conn: &Connection, ttl_secs: i64) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - ttl_secs;
    
    let rows = conn.execute(
        "DELETE FROM tasks WHERE created_at < ?1",
        params![cutoff],
    ).map_err(|e| GscholarError::Database(format!("Cleanup failed: {}", e)))?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::init_tables;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        init_tables(&conn).expect("init tables");
        conn
    }

    #[test]
    fn test_task_crud() {
        let conn = setup_db();
        
        // Insert
        let task = Task::new("machine learning", "openalex");
        insert(&conn, &task).expect("insert");

        // Get
        let fetched = get_by_id(&conn, &task.id).expect("get").expect("found");
        assert_eq!(fetched.keyword, Some("machine learning".to_string()));

        // Update progress
        update_progress(&conn, &task.id, "Searching", 50).expect("update");
        let updated = get_by_id(&conn, &task.id).expect("get").expect("found");
        assert_eq!(updated.progress.percent, 50);

        // Complete
        let result = TaskResult {
            total_papers: 100,
            filtered_papers: 50,
            data: serde_json::json!([]),
            csv_path: Some("/tmp/test.csv".to_string()),
        };
        complete(&conn, &task.id, &result).expect("complete");
        let completed = get_by_id(&conn, &task.id).expect("get").expect("found");
        assert_eq!(completed.status, TaskStatus::Completed);

        // Delete
        assert!(delete(&conn, &task.id).expect("delete"));
        assert!(get_by_id(&conn, &task.id).expect("get").is_none());
    }

    #[test]
    fn test_task_list() {
        let conn = setup_db();
        
        for i in 0..5 {
            let task = Task::new(&format!("keyword {}", i), "openalex");
            insert(&conn, &task).expect("insert");
        }

        let (tasks, total) = list(&conn, 1, 10, None).expect("list");
        assert_eq!(tasks.len(), 5);
        assert_eq!(total, 5);
    }
}
