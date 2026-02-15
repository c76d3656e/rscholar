//! Task recovery on server startup.
//!
//! Handles recovery of interrupted tasks when server restarts.
//! Tasks that were in RUNNING state when server crashed are marked as FAILED
//! so users can retry them.

use crate::db::{tasks as db_tasks, DbPool};
use tracing::{info, warn};

/// Recover interrupted tasks on startup
/// 
/// Strategy: Tasks in RUNNING status are marked as FAILED with a restart message.
/// This handles the case where server crashed/restarted mid-execution.
/// 
/// Returns the number of tasks recovered.
pub async fn recover_interrupted_tasks(db: &DbPool) -> usize {
    // Query running tasks from DB
    let running_tasks = match db.get().await {
        Ok(conn) => {
            match conn.interact(|conn| {
                db_tasks::list(conn, 1, 100, Some("running"))
            }).await {
                Ok(Ok((tasks, _))) => tasks,
                Ok(Err(e)) => {
                    warn!(error = %e, "Failed to query running tasks");
                    return 0;
                }
                Err(e) => {
                    warn!(error = %e, "DB interact failed during recovery");
                    return 0;
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to get DB connection for recovery");
            return 0;
        }
    };

    let count = running_tasks.len();

    if count == 0 {
        info!("No interrupted tasks to recover");
        return 0;
    }

    info!(count = count, "Recovering interrupted tasks (marking as failed)");

    let mut recovered = 0;
    for task in running_tasks {
        let task_id = task.id.clone();
        let reason = "Server restarted during execution. Please submit a new request to retry.";
        
        match db.get().await {
            Ok(conn) => {
                let task_id_clone = task_id.clone();
                let reason_owned = reason.to_string();
                match conn.interact(move |conn| {
                    db_tasks::fail(conn, &task_id_clone, &reason_owned)
                }).await {
                    Ok(Ok(())) => {
                        warn!(task_id = %task_id, "Marked interrupted task as failed");
                        recovered += 1;
                    }
                    Ok(Err(e)) => {
                        warn!(task_id = %task_id, error = %e, "Failed to mark task as failed");
                    }
                    Err(e) => {
                        warn!(task_id = %task_id, error = %e, "DB interact failed");
                    }
                }
            }
            Err(e) => {
                warn!(task_id = %task_id, error = %e, "Failed to get DB connection");
            }
        }
    }

    info!(recovered = recovered, "Task recovery complete");
    recovered
}

#[cfg(test)]
mod tests {
    // Integration tests would be added here
}
