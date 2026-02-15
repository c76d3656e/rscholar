//! Server CLI handlers - production API server and admin init.

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

// ============================================================================
// Production API Server
// ============================================================================

/// Run the production API server with full features
pub async fn run_api_server(port_override: Option<u16>, host_override: Option<String>, serve_static: Option<String>) -> Result<()> {
    use rscholar::db::{api_keys, init_pool, DbConfig};
    use rscholar::server::{config::ServerConfig, recovery, routes::create_router, state::AppState};

    // Initialize database
    let db_config = DbConfig::default();
    let db = init_pool(&db_config).map_err(|e| anyhow::anyhow!("Database error: {}", e))?;

    // Load configuration from file
    let mut config =
        ServerConfig::load_from_file("config.toml").map_err(|e| anyhow::anyhow!("Configuration error: {}", e))?;

    // Apply command-line overrides
    if let Some(p) = port_override {
        config.server.port = p;
    }
    if let Some(h) = host_override {
        config.server.host = h;
    }

    let addr = config
        .socket_addr()
        .map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?;

    // Check if admin key exists (async)
    let conn = db.get().await
        .map_err(|e| anyhow::anyhow!("DB connection error: {}", e))?;
    let has_admin = conn.interact(|conn| {
        api_keys::has_admin_key(conn)
    }).await
    .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    drop(conn);

    if !has_admin {
        println!("⚠️  No admin API key found. Run 'Rscholar init-admin' first.");
    }

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║                Rscholar API Server                        ║");
    println!("╠═══════════════════════════════════════════════════════════╣");
    println!(
        "║  Version:     {}                                    ║",
        env!("CARGO_PKG_VERSION")
    );
    println!("║  Endpoint:    http://{}                      ║", addr);
    println!("║  Database:    {}                   ║", db_config.path);
    println!("║  Auth:        Cloudflare WAF (external)                  ║");
    println!(
        "║  Admin API:   {}                                    ║",
        if config.server.admin_enabled { "enabled" } else { "disabled" }
    );
    if let Some(ref dir) = serve_static {
        println!("║  Static:      {} (SPA mode)                     ║", dir);
    }
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    info!(
        host = %config.server.host,
        port = config.server.port,
        db_path = %db_config.path,
        static_dir = ?serve_static,
        "Starting production API server"
    );

    // Initialize LLM filter via centralized LLM module factory
    let llm_filter = rscholar::llm::LlmRelevanceFilter::build_from_config(&config.llm)
        .map_err(|e| anyhow::anyhow!("LLM configuration error: {}", e))?;

    // Initialize ranking microservice once (persistent key pool + FIFO scheduler)
    let ranking_service = if config.easyscholar.keys.is_empty() {
        None
    } else {
        let scheduler_mode = match config.ranking.scheduler_mode.to_lowercase().as_str() {
            "easy_backfill" => rscholar::rankings::SchedulerMode::EasyBackfill,
            other => {
                return Err(anyhow::anyhow!(
                    "Unsupported ranking.scheduler_mode '{}', expected 'easy_backfill'",
                    other
                ));
            }
        };

        let options = rscholar::rankings::RankingServiceOptions {
            queue_capacity: config.ranking.queue_capacity,
            lease_policy: rscholar::rankings::LeasePolicy {
                min_chunk: config.ranking.min_chunk,
                max_chunk: config.ranking.max_chunk,
            },
            max_concurrent_jobs: config.ranking.max_concurrent_jobs,
            scheduler_mode,
            target_duration_sec: config.ranking.target_duration_sec,
            eta_scale: config.ranking.eta_scale,
            heartbeat_ms: config.ranking.heartbeat_ms,
            job_timeout_min_sec: config.ranking.job_timeout_min_sec,
            key_health_policy: rscholar::rankings::KeyHealthPolicy {
                fail_threshold: config.ranking.key_fail_threshold,
                cooldown_secs: config.ranking.key_cooldown_sec,
                stale_ttl_secs: config.ranking.key_stale_ttl_sec,
            },
        };
        let service = rscholar::rankings::RankingService::new(
            &config.easyscholar.keys,
            Some(db.clone()),
            options,
        )
        .map_err(|e| anyhow::anyhow!("Ranking service initialization error: {}", e))?;
        Some(Arc::new(service))
    };

    // Create application state with database
    let state = AppState::new(config.clone(), db, llm_filter, ranking_service);
    let task_store = state.task_store.clone();

    // Recover interrupted tasks (mark RUNNING tasks as FAILED after restart)
    let recovered = recovery::recover_interrupted_tasks(&state.db).await;
    if recovered > 0 {
        info!(count = recovered, "Recovered interrupted tasks");
    }

    // Create router with all middleware (with optional static file serving)
    let app = create_router(state, serve_static.clone());

    // Start cleanup task for memory cache
    // Uses cleanup_completed with 10 min TTL for completed/failed tasks
    let cleanup_store = task_store.clone();
    let completed_ttl = 600; // 10 minutes for completed tasks
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_store.cleanup_completed(completed_ttl);
        }
    });

    // Bind and serve
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(addr = %addr, "Server listening");

    // Graceful shutdown on Ctrl+C
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    info!("Server shutdown complete");
    Ok(())
}

/// Wait for shutdown signal (Ctrl+C)
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, stopping server...");
}

// ============================================================================
// Admin Key Initialization
// ============================================================================

/// Initialize the first admin API key
pub async fn init_admin_key(name: &str) -> Result<()> {
    use rscholar::db::{api_keys, init_pool, DbConfig};

    println!("Initializing admin API key...\n");

    // Initialize database
    let db_config = DbConfig::default();
    let db = init_pool(&db_config).map_err(|e| anyhow::anyhow!("Database error: {}", e))?;

    let conn = db.get().await
        .map_err(|e| anyhow::anyhow!("DB connection error: {}", e))?;

    // Check if admin already exists
    let has_admin = conn.interact(|conn| {
        api_keys::has_admin_key(conn)
    }).await
    .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    
    if has_admin {
        println!("⚠️  An admin key already exists.");
        println!("   Use the admin endpoints to manage additional keys.");
        return Ok(());
    }

    // Create admin key
    let key_name = name.to_string();
    let created = conn.interact(move |conn| {
        api_keys::create(conn, &key_name, true, 1000)
    }).await
    .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Failed to create key: {}", e))?;

    use secrecy::ExposeSecret;
    
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                    ADMIN API KEY CREATED                           ║");
    println!("╠═══════════════════════════════════════════════════════════════════╣");
    println!("║  Name:       {}                                         ║", created.name);
    println!("║  ID:         {}                                   ║", created.id);
    println!("║                                                                   ║");
    println!("║  🔑 API Key: {}  ║", created.key.expose_secret());
    println!("║                                                                   ║");
    println!("║  ⚠️  SAVE THIS KEY! It will not be shown again.                    ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use this key in the X-API-Key header for admin endpoints.");

    Ok(())
}
