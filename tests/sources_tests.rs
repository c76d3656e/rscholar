//! Data Sources Tests

use rscholar::openalex::{OpenAlexResult, QueryOptions};

// ============================================================================
// OpenAlex Tests
// ============================================================================

#[test]
fn test_query_options_default() {
    let options = QueryOptions::default();
    
    assert_eq!(options.pages, vec![1]);
    assert!(options.ylo.is_none());
}

#[test]
fn test_query_options_custom() {
    let options = QueryOptions {
        pages: vec![1, 2, 3],
        ylo: Some(2020),
        yhi: Some(2024),
        all_results: true,
        ..Default::default()
    };
    
    assert_eq!(options.pages.len(), 3);
    assert_eq!(options.ylo, Some(2020));
    assert_eq!(options.yhi, Some(2024));
    assert!(options.all_results);
}

#[test]
fn test_openalex_result_default() {
    let result = OpenAlexResult::default();
    
    assert!(result.title.is_empty());
    assert!(result.author.is_empty());
    assert!(result.article_url.is_empty());
}

#[test]
fn test_openalex_result_clone() {
    let result = OpenAlexResult {
        title: "Test Title".to_string(),
        author: "Author A, Author B".to_string(),
        article_url: "https://example.com".to_string(),
        snippet: "Abstract preview".to_string(),
        year: "2023".to_string(),
        venue: "Nature".to_string(),
        ..Default::default()
    };
    
    let cloned = result.clone();
    assert_eq!(cloned.title, result.title);
    assert_eq!(cloned.year, result.year);
}

// ============================================================================
// Batch Operation Tests
// ============================================================================

#[test]
fn test_batch_chunking_small() {
    let dois: Vec<String> = (0..50).map(|i| format!("10.1234/{}", i)).collect();
    let chunks: Vec<Vec<String>> = dois.chunks(500).map(|c| c.to_vec()).collect();
    
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].len(), 50);
}

#[test]
fn test_batch_chunking_large() {
    let dois: Vec<String> = (0..1200).map(|i| format!("10.1234/{}", i)).collect();
    let chunks: Vec<Vec<String>> = dois.chunks(500).map(|c| c.to_vec()).collect();
    
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].len(), 500);
    assert_eq!(chunks[1].len(), 500);
    assert_eq!(chunks[2].len(), 200);
}
