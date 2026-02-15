//! Async task queue with dashmap storage.
//!
//! Provides in-memory task storage for long-running pipeline jobs.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Unique task identifier
pub type TaskId = String;

/// Task status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Task is queued, waiting to start
    Pending,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed,
}

/// Task progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    /// Current step description
    pub step: String,
    /// Progress percentage (0-100)
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
    /// Total papers found
    pub total_papers: usize,
    /// Papers after filtering
    pub filtered_papers: usize,
    /// Result data (JSON serialized)
    pub data: serde_json::Value,
    /// CSV file path (if generated)
    pub csv_path: Option<String>,
    /// Per-source paper counts (before merge/dedup)
    pub source_counts: std::collections::HashMap<String, usize>,
    /// Per-source error messages (sources that failed)
    pub source_errors: std::collections::HashMap<String, String>,
}

/// A task in the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID
    pub id: TaskId,
    /// Current status
    pub status: TaskStatus,
    /// Progress information
    pub progress: TaskProgress,
    /// Result (when completed)
    pub result: Option<TaskResult>,
    /// Error message (when failed)
    pub error: Option<String>,
    /// Creation timestamp (Unix epoch seconds)
    pub created_at: u64,
    /// Completion timestamp (Unix epoch seconds) - for TTL cleanup
    pub completed_at: Option<u64>,
    /// Estimated time to completion in seconds
    pub eta_seconds: Option<u64>,
}

impl Task {
    /// Create a new pending task
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            id,
            status: TaskStatus::Pending,
            progress: TaskProgress::default(),
            result: None,
            error: None,
            created_at,
            completed_at: None,
            eta_seconds: Some(120), // Default ETA: 2 minutes
        }
    }

    /// Create a new task with specific ID (useful for testing)
    pub fn with_id(id: String) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            id,
            status: TaskStatus::Pending,
            progress: TaskProgress::default(),
            result: None,
            error: None,
            created_at,
            completed_at: None,
            eta_seconds: Some(120),
        }
    }

    /// Create task from database entity
    pub fn from_db_task(db_task: &crate::db::tasks::Task) -> Self {
        use crate::db::tasks::TaskStatus as DbStatus;
        
        let status = match db_task.status {
            DbStatus::Pending => TaskStatus::Pending,
            DbStatus::Running => TaskStatus::Running,
            DbStatus::Completed => TaskStatus::Completed,
            DbStatus::Failed => TaskStatus::Failed,
        };

        let result = db_task.result.as_ref().map(|r| TaskResult {
            total_papers: r.total_papers,
            filtered_papers: r.filtered_papers,
            data: r.data.clone(),
            csv_path: r.csv_path.clone(),
            source_counts: std::collections::HashMap::new(),
            source_errors: std::collections::HashMap::new(),
        });

        Self {
            id: db_task.id.clone(),
            status,
            progress: TaskProgress {
                step: db_task.progress.step.clone(),
                percent: db_task.progress.percent,
            },
            result,
            error: db_task.error.clone(),
            created_at: db_task.created_at as u64,
            completed_at: if matches!(db_task.status, DbStatus::Completed | DbStatus::Failed) {
                Some(db_task.updated_at as u64)
            } else {
                None
            },
            eta_seconds: None,
        }
    }

    /// Update progress
    pub fn update_progress(&mut self, step: &str, percent: u8) {
        self.progress.step = step.to_string();
        self.progress.percent = percent.min(100);
        if self.status == TaskStatus::Pending {
            self.status = TaskStatus::Running;
        }
    }

    /// Mark as completed with result
    pub fn complete(&mut self, result: TaskResult) {
        self.status = TaskStatus::Completed;
        self.progress.step = "Completed".to_string();
        self.progress.percent = 100;
        self.result = Some(result);
        self.eta_seconds = Some(0);
        self.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
    }

    /// Mark as failed with error
    pub fn fail(&mut self, error: String) {
        self.status = TaskStatus::Failed;
        self.progress.step = "Failed".to_string();
        self.error = Some(error);
        self.eta_seconds = Some(0);
        self.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
    }
}

impl Default for Task {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe task storage using DashMap
#[derive(Clone)]
pub struct TaskStore {
    tasks: Arc<DashMap<TaskId, Task>>,
    created_at: Instant,
}

impl TaskStore {
    /// Create a new task store
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(DashMap::new()),
            created_at: Instant::now(),
        }
    }

    /// Insert a new task
    pub fn insert(&self, task: Task) -> TaskId {
        let id = task.id.clone();
        self.tasks.insert(id.clone(), task);
        debug!(task_id = %id, "Task created");
        id
    }

    /// Get a task by ID
    pub fn get(&self, id: &str) -> Option<Task> {
        self.tasks.get(id).map(|r| r.clone())
    }

    /// Update a task
    pub fn update<F>(&self, id: &str, f: F) -> bool
    where
        F: FnOnce(&mut Task),
    {
        if let Some(mut task) = self.tasks.get_mut(id) {
            f(&mut task);
            true
        } else {
            false
        }
    }

    /// Remove a task
    pub fn remove(&self, id: &str) -> Option<Task> {
        self.tasks.remove(id).map(|(_, t)| t)
    }

    /// Get number of tasks
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Cleanup completed/failed tasks based on TTL after completion
    /// 
    /// Running tasks are never cleaned up (they're still executing).
    /// Only completed or failed tasks are removed after ttl_secs from completion.
    pub fn cleanup_completed(&self, ttl_secs: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut removed = 0;
        self.tasks.retain(|_, task| {
            // Keep running/pending tasks
            if task.status == TaskStatus::Pending || task.status == TaskStatus::Running {
                return true;
            }
            
            // For completed/failed tasks, check TTL from completion time
            if let Some(completed_at) = task.completed_at {
                let age = now.saturating_sub(completed_at);
                if age >= ttl_secs {
                    removed += 1;
                    return false;
                }
            }
            true
        });

        if removed > 0 {
            info!(removed = removed, ttl_secs = ttl_secs, "Cleaned up completed tasks from memory");
        }
    }

    /// Legacy cleanup based on creation time (for backwards compatibility)
    pub fn cleanup(&self, ttl_secs: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut removed = 0;
        self.tasks.retain(|_, task| {
            let age = now.saturating_sub(task.created_at);
            let keep = age < ttl_secs;
            if !keep {
                removed += 1;
            }
            keep
        });

        if removed > 0 {
            info!(removed = removed, ttl_secs = ttl_secs, "Cleaned up old tasks");
        }
    }

    /// Get uptime since store creation
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = Task::new();
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.result.is_none());
        assert!(task.error.is_none());
    }

    #[test]
    fn test_task_progress() {
        let mut task = Task::new();
        task.update_progress("Searching", 25);
        assert_eq!(task.status, TaskStatus::Running);
        assert_eq!(task.progress.step, "Searching");
        assert_eq!(task.progress.percent, 25);
    }

    #[test]
    fn test_task_completion() {
        let mut task = Task::new();
        let result = TaskResult {
            total_papers: 100,
            filtered_papers: 50,
            data: serde_json::json!([]),
            csv_path: Some("/tmp/test.csv".to_string()),
            source_counts: std::collections::HashMap::new(),
            source_errors: std::collections::HashMap::new(),
        };
        task.complete(result);
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.result.is_some());
    }

    #[test]
    fn test_task_failure() {
        let mut task = Task::new();
        task.fail("Connection timeout".to_string());
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.error, Some("Connection timeout".to_string()));
    }

    #[test]
    fn test_task_store() {
        let store = TaskStore::new();
        let task = Task::new();
        let id = task.id.clone();

        store.insert(task);
        assert_eq!(store.len(), 1);

        let retrieved = store.get(&id);
        assert!(retrieved.is_some());

        store.update(&id, |t| t.update_progress("Test", 50));
        let updated = store.get(&id);
        assert_eq!(updated.map(|t| t.progress.percent), Some(50));

        store.remove(&id);
        assert!(store.is_empty());
    }
}
