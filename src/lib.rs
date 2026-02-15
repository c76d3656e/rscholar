//! # Rscholar
#![forbid(unsafe_code)]
//!
//! Academic literature search and filter pipeline - Rust Microservice
//!
//! ## Modules
//!
//! ### Data Sources (`sources/`)
//! - [`openalex`] - OpenAlex API client (recommended)
//! - [`crossref`] - Crossref metadata enrichment
//! - [`semanticscholar`] - Semantic Scholar abstracts and PDFs
//! - [`arxiv`] - arXiv Atom search API
//! - [`pubmed`] - PubMed E-utilities search + abstract fetch
//! - [`xrxiv`] - bioRxiv/medRxiv preprint search
//!
//! ### Processing

//! - [`rankings`] - EasyScholar journal ranking
//! - [`unified`] - Unified output format
//!
//! ### Infrastructure
//! - [`server`] - HTTP API server
//! - [`db`] - SQLite database (tasks, API keys, cache, analytics)
//!
//! ## Pipeline Flow
//!
//! 1. Search (OpenAlex + Semantic Scholar)
//! 2. Semantic Scholar enrichment

//! 4. EasyScholar ranking
//! 5. Unified output
//!
//! ## Usage
//!
//! ```rust,no_run
//! use rscholar::{openalex, rankings};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let results = openalex::query("machine learning", &Default::default()).await?;
//!     println!("Found {} results", results.len());
//!     Ok(())
//! }
//! ```

// Core modules
pub mod db;
pub mod error;
pub mod ranking;
pub use ranking as rankings; // Backward compat alias

pub mod server;
pub mod traffic;
pub mod unified;
pub mod llm;

// Data sources (reorganized)
pub mod sources;
pub use sources::arxiv;
pub use sources::crossref;
pub use sources::openalex;
pub use sources::pubmed;
pub use sources::semanticscholar;
pub use sources::xrxiv;

pub use error::{GscholarError, Result};
