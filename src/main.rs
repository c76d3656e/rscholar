//! Rscholar - Academic literature search and filter pipeline
#![forbid(unsafe_code)]
//!
//! Supports OpenAlex search, Semantic Scholar enrichment,
//! SiliconFlow Rerank relevance sorting, EasyScholar journal ranking.
//!
//! ## CLI Mode
//! ```bash
//! Rscholar search "deep learning" --source openalex --pages 1-3
//! ```
//!
//! ## HTTP Server Mode
//! ```bash
//! Rscholar server --port 3000
//! ```

mod cli;

use anyhow::Result;
use clap::Parser;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let parsed_cli = cli::Cli::parse();

    // Initialize logging
    let log_level = if parsed_cli.debug {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level.to_string()));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    info!(
        debug = parsed_cli.debug,
        "Rscholar CLI initialized"
    );

    match parsed_cli.command {
        cli::Commands::Search {
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

        } => {
            info!(
                keyword = %keyword,
                source = %source,
                pages = %pages,
                "Executing search command"
            );
            let args = cli::SearchArgs::new(
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

            );
            cli::run_search_pipeline(args).await
        }
        cli::Commands::Server { port, host, serve_static } => {
            info!(port = ?port, host = ?host, serve_static = ?serve_static, "Executing server command");
            cli::run_api_server(port, host, serve_static).await
        }
        cli::Commands::InitAdmin { name } => {
            info!(name = %name, "Executing init-admin command");
            cli::init_admin_key(&name).await
        }
    }
}
