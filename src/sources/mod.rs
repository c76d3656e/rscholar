//! Data sources for academic literature search.
//!
//! This module provides clients for various academic data sources:
//! - OpenAlex (open API)
//! - Semantic Scholar (DOI lookups and search)
//! - Crossref (metadata enrichment)
//! - arXiv / PubMed / bioRxiv / medRxiv (configurable source fan-out)

use crate::error::Result;
use async_trait::async_trait;

pub mod arxiv;
pub mod crossref;
pub mod openalex;
pub mod pubmed;
pub mod rate_limiter;
pub mod semanticscholar;
pub mod xrxiv;

// Re-export main types for convenience
pub use arxiv::{search_papers as arxiv_search, ArxivQueryOptions, ArxivResult};
pub use crossref::CrossrefClient;
pub use openalex::{query as openalex_query, OpenAlexResult, QueryOptions as OpenAlexOptions};
pub use pubmed::{search_papers as pubmed_search, PubMedQueryOptions, PubMedResult};
pub use semanticscholar::{batch_lookup as ss_batch_lookup, SemanticScholarResult};
pub use xrxiv::{search_papers as xrxiv_search, XRxivQueryOptions, XRxivResult, XRxivServer};

/// Unified normalized paper payload returned by source providers.
///
/// This struct allows each source to keep provider-specific request/response logic
/// while exposing a stable schema for pipeline stage mapping.
#[derive(Debug, Clone, Default)]
pub struct SourcePaper {
    pub title: String,
    pub authors: String,
    pub year: String,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub pdf_url: String,
    pub snippet: String,
    pub abstract_text: String,
}

/// Unified trait for source providers consumed by pipeline search stage.
#[async_trait]
pub trait SourceProvider<TOptions>: Send + Sync {
    fn source_name(&self) -> &'static str;
    async fn search(&self, query: &str, options: &TOptions) -> Result<Vec<SourcePaper>>;
}
