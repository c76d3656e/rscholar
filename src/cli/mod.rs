//! CLI module - Command line interface definitions and handlers.
//!
//! This module contains:
//! - CLI argument parsing (clap)
//! - Command handlers for each subcommand

mod search;
mod server;

pub use search::run_search_pipeline;
pub use server::{init_admin_key, run_api_server};

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::debug;

/// Rscholar - Academic literature search and filter pipeline
#[derive(Parser)]
#[command(name = "Rscholar")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable debug logging
    #[arg(short, long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search academic literature and run the pipeline
    Search {
        /// Search keywords
        keyword: String,

        /// Search source (currently supports openalex)
        #[arg(long, default_value = "openalex", value_parser = ["openalex"])]
        source: String,

        /// Page range (e.g., "1", "1-5", "1-10")
        #[arg(long, default_value = "1-5")]
        pages: String,

        /// Year filter (results from this year onwards)
        #[arg(long)]
        ylo: Option<i32>,

        /// Output directory
        #[arg(short, long, default_value = "./output")]
        output: PathBuf,

        // === EasyScholar Filters ===
        /// EasyScholar API key (required for filtering)
        #[arg(long)]
        easyscholar_key: Option<String>,

        /// Filter: Impact Factor >= value
        #[arg(long)]
        sciif: Option<f64>,

        /// Filter: JCI >= value
        #[arg(long)]
        jci: Option<f64>,

        /// Filter: SCI partition (e.g., "Q1")
        #[arg(long)]
        sci: Option<String>,

        /// Filter: sciUpTop (substring match)
        #[arg(long)]
        sci_up_top: Option<String>,

        /// Filter: sciBase (substring match)
        #[arg(long)]
        sci_base: Option<String>,

        /// Filter: sciUp (substring match)
        #[arg(long)]
        sci_up: Option<String>,
    },


    /// Run as HTTP API server (production)
    Server {
        /// Port to listen on (overrides RUSTGSCHOLAR_PORT env var)
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to (overrides RUSTGSCHOLAR_HOST env var)
        #[arg(long)]
        host: Option<String>,

        /// Serve static files from directory (e.g., "./front/dist")
        #[arg(long)]
        serve_static: Option<String>,
    },
    /// Initialize admin API key (run once on first setup)
    InitAdmin {
        /// Name for the admin key
        #[arg(long, default_value = "Admin")]
        name: String,
    },
}

/// Search command arguments (collected for cleaner function signature)
#[derive(Debug, Clone)]
pub struct SearchArgs {
    pub keyword: String,
    pub source: String,
    pub pages: String,
    pub ylo: Option<i32>,
    pub output: PathBuf,
    pub easyscholar_key: Option<String>,
    pub sciif: Option<f64>,
    pub jci: Option<f64>,
    pub sci: Option<String>,
    pub sci_up_top: Option<String>,
    pub sci_base: Option<String>,
    pub sci_up: Option<String>,

}

impl SearchArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        keyword: String,
        source: String,
        pages: String,
        ylo: Option<i32>,
        output: PathBuf,
        easyscholar_key: Option<String>,
        sciif: Option<f64>,
        jci: Option<f64>,
        sci: Option<String>,
        sci_up_top: Option<String>,
        sci_base: Option<String>,
        sci_up: Option<String>,

    ) -> Self {
        debug!(
            keyword = %keyword,
            source = %source,
            pages = %pages,
            has_easyscholar_key = easyscholar_key.is_some(),
            has_filters = sciif.is_some() || jci.is_some() || sci.is_some() || sci_up_top.is_some() || sci_base.is_some() || sci_up.is_some(),
            "Constructed search arguments"
        );
        Self {
            keyword,
            source,
            pages,
            ylo,
            output,
            easyscholar_key,
            sciif,
            jci,
            sci,
            sci_up_top,
            sci_base,
            sci_up,

        }
    }
}
