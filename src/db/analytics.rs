//! Usage analytics and statistics.
//!
//! Logging and querying search behavior data.

use crate::error::{GscholarError, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use tracing::debug;

/// Log a search event
pub fn log_search(
    conn: &Connection,
    api_key_id: Option<&str>,
    keyword: &str,
    source: Option<&str>,
    result_count: i32,
    journals: &[String],
) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO search_logs (api_key_id, keyword, source, result_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![api_key_id, keyword, source, result_count, now],
    ).map_err(|e| GscholarError::Database(format!("Log search failed: {}", e)))?;

    let search_id = conn.last_insert_rowid();

    // Insert journal hits
    for journal in journals {
        if !journal.is_empty() {
            conn.execute(
                "INSERT INTO journal_hits (search_id, journal_name) VALUES (?1, ?2)",
                params![search_id, journal],
            ).ok();
        }
    }

    debug!(search_id = search_id, keyword = %keyword, journals = journals.len(), "Search logged");
    Ok(search_id)
}

/// Overview statistics
#[derive(Debug, Clone, Serialize)]
pub struct StatsOverview {
    pub total_searches: i64,
    pub total_papers_returned: i64,
    pub unique_keywords: i64,
    pub unique_journals: i64,
    pub avg_papers_per_search: f64,
}

pub fn get_overview(conn: &Connection) -> Result<StatsOverview> {
    let total_searches: i64 = conn.query_row(
        "SELECT COUNT(*) FROM search_logs",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_papers: i64 = conn.query_row(
        "SELECT COALESCE(SUM(result_count), 0) FROM search_logs",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let unique_keywords: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT keyword) FROM search_logs",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let unique_journals: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT journal_name) FROM journal_hits",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let avg_papers = if total_searches > 0 {
        total_papers as f64 / total_searches as f64
    } else {
        0.0
    };

    Ok(StatsOverview {
        total_searches,
        total_papers_returned: total_papers,
        unique_keywords,
        unique_journals,
        avg_papers_per_search: avg_papers,
    })
}

/// Keyword statistics
#[derive(Debug, Clone, Serialize)]
pub struct KeywordStats {
    pub keyword: String,
    pub count: i64,
    pub avg_results: f64,
}

pub fn get_top_keywords(conn: &Connection, limit: u32) -> Result<Vec<KeywordStats>> {
    let mut stmt = conn.prepare(
        "SELECT keyword, COUNT(*) as cnt, AVG(result_count) as avg_res
         FROM search_logs
         GROUP BY keyword
         ORDER BY cnt DESC
         LIMIT ?1"
    ).map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = stmt.query_map(params![limit], |row| {
        Ok(KeywordStats {
            keyword: row.get(0)?,
            count: row.get(1)?,
            avg_results: row.get(2)?,
        })
    }).map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let stats: Vec<KeywordStats> = rows.filter_map(|r| r.ok()).collect();
    Ok(stats)
}

/// Journal statistics
#[derive(Debug, Clone, Serialize)]
pub struct JournalStats {
    pub journal_name: String,
    pub count: i64,
}

pub fn get_top_journals(conn: &Connection, limit: u32) -> Result<Vec<JournalStats>> {
    let mut stmt = conn.prepare(
        "SELECT journal_name, COUNT(*) as cnt
         FROM journal_hits
         GROUP BY journal_name
         ORDER BY cnt DESC
         LIMIT ?1"
    ).map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = stmt.query_map(params![limit], |row| {
        Ok(JournalStats {
            journal_name: row.get(0)?,
            count: row.get(1)?,
        })
    }).map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let stats: Vec<JournalStats> = rows.filter_map(|r| r.ok()).collect();
    Ok(stats)
}

/// Daily statistics
#[derive(Debug, Clone, Serialize)]
pub struct DailyStats {
    pub date: String,
    pub searches: i64,
    pub papers: i64,
}

pub fn get_daily_stats(conn: &Connection, days: u32) -> Result<Vec<DailyStats>> {
    let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);

    let mut stmt = conn.prepare(
        "SELECT date(created_at, 'unixepoch') as day, COUNT(*), SUM(result_count)
         FROM search_logs
         WHERE created_at >= ?1
         GROUP BY day
         ORDER BY day DESC"
    ).map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = stmt.query_map(params![cutoff], |row| {
        Ok(DailyStats {
            date: row.get(0)?,
            searches: row.get(1)?,
            papers: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }).map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let stats: Vec<DailyStats> = rows.filter_map(|r| r.ok()).collect();
    Ok(stats)
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
    fn test_log_search() {
        let conn = setup_db();
        
        let journals = vec!["Nature".to_string(), "Science".to_string()];
        let id = log_search(&conn, Some("key1"), "AI", Some("openalex"), 50, &journals)
            .expect("log");
        
        assert!(id > 0);

        let overview = get_overview(&conn).expect("overview");
        assert_eq!(overview.total_searches, 1);
        assert_eq!(overview.total_papers_returned, 50);
    }

    #[test]
    fn test_top_keywords() {
        let conn = setup_db();
        
        log_search(&conn, None, "AI", None, 10, &[]).expect("log");
        log_search(&conn, None, "AI", None, 20, &[]).expect("log");
        log_search(&conn, None, "ML", None, 15, &[]).expect("log");

        let top = get_top_keywords(&conn, 10).expect("top");
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].keyword, "AI");
        assert_eq!(top[0].count, 2);
    }

    #[test]
    fn test_top_journals() {
        let conn = setup_db();
        
        let journals1 = vec!["Nature".to_string(), "Science".to_string()];
        let journals2 = vec!["Nature".to_string(), "Cell".to_string()];
        
        log_search(&conn, None, "test1", None, 10, &journals1).expect("log");
        log_search(&conn, None, "test2", None, 10, &journals2).expect("log");

        let top = get_top_journals(&conn, 10).expect("top");
        assert_eq!(top[0].journal_name, "Nature");
        assert_eq!(top[0].count, 2);
    }
}
