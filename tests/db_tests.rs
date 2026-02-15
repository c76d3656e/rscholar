//! Database Integration Tests
//!
//! Tests all database modules with real SQLite operations.

mod common;

use rscholar::db::{api_keys, analytics, journal_cache, tasks};
use secrecy::ExposeSecret;

// ============================================================================
// API Keys Tests
// ============================================================================

#[test]
fn test_api_keys_create_and_validate() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let created = api_keys::create(&conn, "Test Key", false, 10)
        .expect("Failed to create key");
    
    assert!(created.key.expose_secret().starts_with("rgs_"));
    assert_eq!(created.name, "Test Key");
    
    let validated = api_keys::validate(&conn, created.key.expose_secret())
        .expect("Failed to validate key");
    
    assert!(validated.is_some());
    let key = validated.expect("Key should exist");
    assert_eq!(key.name, "Test Key");
}

#[test]
fn test_api_keys_invalid_key() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let result = api_keys::validate(&conn, "invalid-key-12345")
        .expect("Validate should not error");
    
    assert!(result.is_none());
}

#[test]
fn test_api_keys_list_and_count() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    api_keys::create(&conn, "Key 1", false, 10).expect("create 1");
    api_keys::create(&conn, "Key 2", true, 20).expect("create 2");
    api_keys::create(&conn, "Key 3", false, 30).expect("create 3");
    
    let (keys, total) = api_keys::list(&conn, 1, 10).expect("list");
    
    assert_eq!(total, 3);
    assert_eq!(keys.len(), 3);
}

#[test]
fn test_api_keys_update() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let created = api_keys::create(&conn, "Original", false, 10).expect("create");
    
    let updated = api_keys::update(&conn, &created.id, Some("Updated"), Some(50))
        .expect("update");
    assert!(updated);
    
    let key = api_keys::get_by_id(&conn, &created.id)
        .expect("get")
        .expect("found");
    assert_eq!(key.name, "Updated");
    assert_eq!(key.rate_limit_rps, 50);
}

#[test]
fn test_api_keys_delete() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let created = api_keys::create(&conn, "ToDelete", false, 10).expect("create");
    let deleted = api_keys::delete(&conn, &created.id).expect("delete");
    assert!(deleted);
    
    let result = api_keys::get_by_id(&conn, &created.id).expect("get");
    assert!(result.is_none());
}

#[test]
fn test_api_keys_has_admin() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    assert!(!api_keys::has_admin_key(&conn).expect("check"));
    api_keys::create(&conn, "Admin", true, 100).expect("create admin");
    assert!(api_keys::has_admin_key(&conn).expect("check"));
}

#[test]
fn test_api_keys_usage_tracking() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let created = api_keys::create(&conn, "Test", false, 10).expect("create");
    let hash = api_keys::hash_key(created.key.expose_secret());
    
    api_keys::record_usage(&conn, &hash).expect("record 1");
    api_keys::record_usage(&conn, &hash).expect("record 2");
    
    let key = api_keys::get_by_id(&conn, &created.id)
        .expect("get")
        .expect("found");
    assert_eq!(key.request_count, 2);
}

// ============================================================================
// Tasks Tests
// ============================================================================

#[test]
fn test_tasks_create_and_get() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let task = tasks::Task::new("deep learning", "openalex");
    let task_id = task.id.clone();
    
    tasks::insert(&conn, &task).expect("insert");
    
    let fetched = tasks::get_by_id(&conn, &task_id)
        .expect("get")
        .expect("found");
    assert_eq!(fetched.keyword, Some("deep learning".to_string()));
    assert_eq!(fetched.source, Some("openalex".to_string()));
    assert_eq!(fetched.status, tasks::TaskStatus::Pending);
}

#[test]
fn test_tasks_update_progress() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let task = tasks::Task::new("test", "openalex");
    let task_id = task.id.clone();
    tasks::insert(&conn, &task).expect("insert");
    
    tasks::update_progress(&conn, &task_id, "Searching", 50).expect("update");
    
    let updated = tasks::get_by_id(&conn, &task_id)
        .expect("get")
        .expect("found");
    assert_eq!(updated.progress.step, "Searching");
    assert_eq!(updated.progress.percent, 50);
}

#[test]
fn test_tasks_complete() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let task = tasks::Task::new("test", "openalex");
    let task_id = task.id.clone();
    tasks::insert(&conn, &task).expect("insert");
    
    let result = tasks::TaskResult {
        total_papers: 100,
        filtered_papers: 42,
        data: serde_json::json!([]),
        csv_path: None,
    };
    tasks::complete(&conn, &task_id, &result).expect("complete");
    
    let completed = tasks::get_by_id(&conn, &task_id)
        .expect("get")
        .expect("found");
    assert_eq!(completed.status, tasks::TaskStatus::Completed);
    assert!(completed.result.is_some());
}

#[test]
fn test_tasks_fail() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let task = tasks::Task::new("test", "openalex");
    let task_id = task.id.clone();
    tasks::insert(&conn, &task).expect("insert");
    
    tasks::fail(&conn, &task_id, "Something went wrong").expect("fail");
    
    let failed = tasks::get_by_id(&conn, &task_id)
        .expect("get")
        .expect("found");
    assert_eq!(failed.status, tasks::TaskStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("Something went wrong"));
}

#[test]
fn test_tasks_list() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    for i in 0..5 {
        let task = tasks::Task::new(&format!("query {}", i), "openalex");
        tasks::insert(&conn, &task).expect("insert");
    }
    
    let (all, total) = tasks::list(&conn, 1, 10, None).expect("list all");
    assert_eq!(total, 5);
    assert_eq!(all.len(), 5);
}

#[test]
fn test_tasks_delete() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let task = tasks::Task::new("to delete", "openalex");
    let task_id = task.id.clone();
    tasks::insert(&conn, &task).expect("insert");
    
    assert!(tasks::delete(&conn, &task_id).expect("delete"));
    assert!(tasks::get_by_id(&conn, &task_id).expect("get").is_none());
}

// ============================================================================
// Journal Cache Tests
// ============================================================================

#[test]
fn test_journal_cache_upsert_and_get() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let entry = journal_cache::JournalRanking {
        name: "Nature".to_string(),
        sciif: Some("69.504".to_string()),
        jci: Some("25.67".to_string()),
        sci: Some("Q1".to_string()),
        sci_up_top: None,
        sci_base: None,
        sci_up: None,
        fetched_at: chrono::Utc::now().timestamp(),
    };
    
    journal_cache::upsert(&conn, &entry).expect("upsert");
    
    let cached = journal_cache::get(&conn, "Nature")
        .expect("get")
        .expect("found");
    assert_eq!(cached.sciif.as_deref(), Some("69.504"));
    assert_eq!(cached.sci.as_deref(), Some("Q1"));
}

#[test]
fn test_journal_cache_list() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    for name in ["Nature", "Science", "Cell", "PNAS"] {
        let entry = journal_cache::JournalRanking {
            name: name.to_string(),
            sciif: Some("10.0".to_string()),
            jci: None,
            sci: None,
            sci_up_top: None,
            sci_base: None,
            sci_up: None,
            fetched_at: chrono::Utc::now().timestamp(),
        };
        journal_cache::upsert(&conn, &entry).expect("upsert");
    }
    
    let (journals, total) = journal_cache::list(&conn, 1, 10).expect("list");
    assert_eq!(total, 4);
    assert_eq!(journals.len(), 4);
}

#[test]
fn test_journal_cache_delete() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let entry = journal_cache::JournalRanking {
        name: "ToDelete".to_string(),
        sciif: Some("5.0".to_string()),
        jci: None,
        sci: None,
        sci_up_top: None,
        sci_base: None,
        sci_up: None,
        fetched_at: chrono::Utc::now().timestamp(),
    };
    journal_cache::upsert(&conn, &entry).expect("upsert");
    
    assert!(journal_cache::delete(&conn, "ToDelete").expect("delete"));
    assert!(journal_cache::get(&conn, "ToDelete").expect("get").is_none());
}

#[test]
fn test_journal_cache_stats() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    for i in 0..10 {
        let entry = journal_cache::JournalRanking {
            name: format!("Journal {}", i),
            sciif: Some("5.0".to_string()),
            jci: None,
            sci: None,
            sci_up_top: None,
            sci_base: None,
            sci_up: None,
            fetched_at: chrono::Utc::now().timestamp(),
        };
        journal_cache::upsert(&conn, &entry).expect("upsert");
    }
    
    let stats = journal_cache::get_stats(&conn).expect("stats");
    assert_eq!(stats.total_entries, 10);
}

// ============================================================================
// Analytics Tests
// ============================================================================

#[test]
fn test_analytics_log_search() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let journals = vec!["Nature".to_string(), "Science".to_string()];
    analytics::log_search(&conn, None, "deep learning", Some("openalex"), 50, &journals)
        .expect("log");
    
    let overview = analytics::get_overview(&conn).expect("overview");
    assert_eq!(overview.total_searches, 1);
    assert_eq!(overview.total_papers_returned, 50);
}

#[test]
fn test_analytics_top_keywords() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    analytics::log_search(&conn, None, "AI", None, 10, &[]).expect("log");
    analytics::log_search(&conn, None, "AI", None, 20, &[]).expect("log");
    analytics::log_search(&conn, None, "ML", None, 15, &[]).expect("log");
    
    let top = analytics::get_top_keywords(&conn, 10).expect("top");
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].keyword, "AI");
    assert_eq!(top[0].count, 2);
}

#[test]
fn test_analytics_top_journals() {
    let db = common::TestDb::new();
    let conn = db.get_conn();
    
    let journals1 = vec!["Nature".to_string(), "Science".to_string()];
    let journals2 = vec!["Nature".to_string(), "Cell".to_string()];
    
    analytics::log_search(&conn, None, "test1", None, 10, &journals1).expect("log");
    analytics::log_search(&conn, None, "test2", None, 10, &journals2).expect("log");
    
    let top = analytics::get_top_journals(&conn, 10).expect("top");
    assert!(top.len() >= 2);
    assert_eq!(top[0].journal_name, "Nature");
    assert_eq!(top[0].count, 2);
}
