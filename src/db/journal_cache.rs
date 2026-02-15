//! Journal ranking cache.
//!
//! Caches EasyScholar API results to reduce API calls.

use crate::error::{GscholarError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Cached journal ranking data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalRanking {
    pub name: String,
    pub sciif: Option<String>,
    pub jci: Option<String>,
    pub sci: Option<String>,
    pub sci_up_top: Option<String>,
    pub sci_base: Option<String>,
    pub sci_up: Option<String>,
    pub fetched_at: i64,
}

/// Get a cached journal ranking
pub fn get(conn: &Connection, name: &str) -> Result<Option<JournalRanking>> {
    // Update cache stats
    conn.execute(
        "UPDATE cache_stats SET total_lookups = total_lookups + 1 WHERE id = 1",
        [],
    ).ok();

    let ranking = conn.query_row(
        "SELECT name, sciif, jci, sci, sci_up_top, sci_base, sci_up, fetched_at
         FROM journal_cache WHERE name = ?1",
        params![name],
        |row| {
            Ok(JournalRanking {
                name: row.get(0)?,
                sciif: row.get(1)?,
                jci: row.get(2)?,
                sci: row.get(3)?,
                sci_up_top: row.get(4)?,
                sci_base: row.get(5)?,
                sci_up: row.get(6)?,
                fetched_at: row.get(7)?,
            })
        },
    ).optional()
    .map_err(|e| GscholarError::Database(format!("Get cache failed: {}", e)))?;

    if ranking.is_some() {
        // Record cache hit
        conn.execute(
            "UPDATE cache_stats SET cache_hits = cache_hits + 1 WHERE id = 1",
            [],
        ).ok();
        debug!(name = %name, "Journal cache hit");
    }

    Ok(ranking)
}

/// Batch get cached journal rankings (returns HashMap for easy lookup)
pub fn batch_get(conn: &Connection, names: &[String]) -> Result<std::collections::HashMap<String, JournalRanking>> {
    use std::collections::HashMap;
    
    let mut results: HashMap<String, JournalRanking> = HashMap::new();
    
    for name in names {
        if name.is_empty() {
            continue;
        }
        if let Some(ranking) = get(conn, name)? {
            results.insert(name.clone(), ranking);
        }
    }
    
    Ok(results)
}

/// Insert or update a cached journal ranking
pub fn upsert(conn: &Connection, ranking: &JournalRanking) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO journal_cache (name, sciif, jci, sci, sci_up_top, sci_base, sci_up, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ranking.name,
            ranking.sciif,
            ranking.jci,
            ranking.sci,
            ranking.sci_up_top,
            ranking.sci_base,
            ranking.sci_up,
            ranking.fetched_at,
        ],
    ).map_err(|e| GscholarError::Database(format!("Upsert cache failed: {}", e)))?;

    debug!(name = %ranking.name, "Journal cached");
    Ok(())
}

/// Batch insert or update cached journal rankings in one transaction.
pub fn batch_upsert(conn: &Connection, rankings: &[JournalRanking]) -> Result<()> {
    if rankings.is_empty() {
        return Ok(());
    }

    conn.execute_batch("BEGIN IMMEDIATE TRANSACTION")
        .map_err(|e| GscholarError::Database(format!("Begin batch upsert transaction failed: {}", e)))?;

    for ranking in rankings {
        if let Err(error) = conn.execute(
            "INSERT OR REPLACE INTO journal_cache (name, sciif, jci, sci, sci_up_top, sci_base, sci_up, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                ranking.name,
                ranking.sciif,
                ranking.jci,
                ranking.sci,
                ranking.sci_up_top,
                ranking.sci_base,
                ranking.sci_up,
                ranking.fetched_at,
            ],
        ) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(GscholarError::Database(format!(
                "Batch upsert cache failed: {}",
                error
            )));
        }
    }

    conn.execute_batch("COMMIT")
        .map_err(|e| GscholarError::Database(format!("Commit batch upsert transaction failed: {}", e)))?;

    debug!(entries = rankings.len(), "Journal cache batch upsert complete");
    Ok(())
}

/// Delete a single cached entry
pub fn delete(conn: &Connection, name: &str) -> Result<bool> {
    let rows = conn.execute(
        "DELETE FROM journal_cache WHERE name = ?1",
        params![name],
    ).map_err(|e| GscholarError::Database(format!("Delete cache failed: {}", e)))?;

    Ok(rows > 0)
}

/// Clear all cached entries
pub fn clear_all(conn: &Connection) -> Result<usize> {
    let rows = conn.execute("DELETE FROM journal_cache", [])
        .map_err(|e| GscholarError::Database(format!("Clear cache failed: {}", e)))?;

    // Reset cache stats
    conn.execute(
        "UPDATE cache_stats SET total_lookups = 0, cache_hits = 0 WHERE id = 1",
        [],
    ).ok();

    debug!(cleared = rows, "Journal cache cleared");
    Ok(rows)
}

/// List cached journals with pagination
pub fn list(conn: &Connection, page: u32, limit: u32) -> Result<(Vec<JournalRanking>, i64)> {
    let offset = (page.saturating_sub(1)) * limit;

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM journal_cache", [], |row| row.get(0))
        .unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT name, sciif, jci, sci, sci_up_top, sci_base, sci_up, fetched_at
         FROM journal_cache ORDER BY fetched_at DESC LIMIT ?1 OFFSET ?2"
    ).map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(JournalRanking {
            name: row.get(0)?,
            sciif: row.get(1)?,
            jci: row.get(2)?,
            sci: row.get(3)?,
            sci_up_top: row.get(4)?,
            sci_base: row.get(5)?,
            sci_up: row.get(6)?,
            fetched_at: row.get(7)?,
        })
    }).map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let rankings: Vec<JournalRanking> = rows.filter_map(|r| r.ok()).collect();
    Ok((rankings, total))
}

/// Get cache statistics
#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub total_entries: i64,
    pub oldest_entry_at: Option<i64>,
    pub newest_entry_at: Option<i64>,
    pub total_lookups: i64,
    pub cache_hits: i64,
    pub hit_rate: f64,
}

pub fn get_stats(conn: &Connection) -> Result<CacheStats> {
    let total_entries: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal_cache",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let oldest_entry_at: Option<i64> = conn.query_row(
        "SELECT MIN(fetched_at) FROM journal_cache",
        [],
        |row| row.get(0),
    ).ok().flatten();

    let newest_entry_at: Option<i64> = conn.query_row(
        "SELECT MAX(fetched_at) FROM journal_cache",
        [],
        |row| row.get(0),
    ).ok().flatten();

    let (total_lookups, cache_hits): (i64, i64) = conn.query_row(
        "SELECT total_lookups, cache_hits FROM cache_stats WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap_or((0, 0));

    let hit_rate = if total_lookups > 0 {
        cache_hits as f64 / total_lookups as f64
    } else {
        0.0
    };

    Ok(CacheStats {
        total_entries,
        oldest_entry_at,
        newest_entry_at,
        total_lookups,
        cache_hits,
        hit_rate,
    })
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
    fn test_cache_crud() {
        let conn = setup_db();
        
        let ranking = JournalRanking {
            name: "Nature".to_string(),
            sciif: Some("69.504".to_string()),
            jci: Some("5.123".to_string()),
            sci: Some("Q1".to_string()),
            sci_up_top: None,
            sci_base: None,
            sci_up: None,
            fetched_at: chrono::Utc::now().timestamp(),
        };

        // Insert
        upsert(&conn, &ranking).expect("upsert");

        // Get
        let cached = get(&conn, "Nature").expect("get").expect("found");
        assert_eq!(cached.sciif, Some("69.504".to_string()));

        // List
        let (list, total) = list(&conn, 1, 10).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(total, 1);

        // Delete
        assert!(delete(&conn, "Nature").expect("delete"));
        assert!(get(&conn, "Nature").expect("get").is_none());
    }

    #[test]
    fn test_cache_stats() {
        let conn = setup_db();
        
        let ranking = JournalRanking {
            name: "Science".to_string(),
            sciif: Some("50.0".to_string()),
            jci: None,
            sci: None,
            sci_up_top: None,
            sci_base: None,
            sci_up: None,
            fetched_at: chrono::Utc::now().timestamp(),
        };
        upsert(&conn, &ranking).expect("upsert");

        // Miss
        get(&conn, "Unknown").expect("get");
        // Hit
        get(&conn, "Science").expect("get");

        let stats = get_stats(&conn).expect("stats");
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.total_lookups, 2);
        assert_eq!(stats.cache_hits, 1);
        assert!((stats.hit_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_batch_upsert() {
        let conn = setup_db();
        let now = chrono::Utc::now().timestamp();

        let rankings = vec![
            JournalRanking {
                name: "Nature".to_string(),
                sciif: Some("69.504".to_string()),
                jci: Some("5.123".to_string()),
                sci: Some("Q1".to_string()),
                sci_up_top: None,
                sci_base: None,
                sci_up: None,
                fetched_at: now,
            },
            JournalRanking {
                name: "Science".to_string(),
                sciif: Some("63.714".to_string()),
                jci: Some("4.987".to_string()),
                sci: Some("Q1".to_string()),
                sci_up_top: None,
                sci_base: None,
                sci_up: None,
                fetched_at: now,
            },
        ];

        batch_upsert(&conn, &rankings).expect("batch upsert");
        let (list, total) = list(&conn, 1, 10).expect("list");
        assert_eq!(total, 2);
        assert_eq!(list.len(), 2);
    }
}
