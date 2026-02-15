use rscholar::rankings::{RankingBatchRequest, RankingService, RankingServiceOptions};
use rscholar::server::config::ServerConfig;
use std::sync::Arc;
use std::sync::Once;
use uuid::Uuid;
use tracing_subscriber::EnvFilter;

static TEST_LOG_INIT: Once = Once::new();

fn init_test_tracing() {
    TEST_LOG_INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(true)
            .with_test_writer()
            .init();
    });
}

fn load_config() -> ServerConfig {
    ServerConfig::load_from_file("config.toml")
        .expect("failed to load config.toml for ranking live test")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires network and valid easyscholar.keys in config.toml"]
async fn test_ranking_service_live_query_single_journal() {
    init_test_tracing();
    let config = load_config();
    assert!(
        !config.easyscholar.keys.is_empty(),
        "config.toml easyscholar.keys must not be empty"
    );

    let service = RankingService::new(
        &config.easyscholar.keys,
        None,
        RankingServiceOptions::default(),
    )
    .expect("ranking service should initialize from config keys");

    let venue = "Rock Mechanics and Rock Engineering".to_string();
    let request = RankingBatchRequest {
        task_id: "ranking-live-test".to_string(),
        venues: vec![venue.clone()],
    };

    let result = service
        .lookup_batch(request)
        .await
        .expect("live ranking lookup should not fail");

    let metrics = result.by_venue.get(&venue);
    println!("venue: {}", venue);
    println!("stats: cache_hits={}, cache_misses={}, api_hits={}, chunk_size={}",
        result.cache_hits, result.cache_misses, result.api_hits, result.chunk_size_granted);
    println!("metrics: {:?}", metrics);

    assert_eq!(
        result.venue_total, 1,
        "expected exactly one venue to be processed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires network and valid easyscholar.keys in config.toml"]
async fn test_ranking_service_live_schedule_300_random_venues_five_jobs() {
    init_test_tracing();
    let config = load_config();
    assert!(
        !config.easyscholar.keys.is_empty(),
        "config.toml easyscholar.keys must not be empty"
    );

    let scheduler_mode = match config.ranking.scheduler_mode.to_lowercase().as_str() {
        "easy_backfill" => rscholar::rankings::SchedulerMode::EasyBackfill,
        other => panic!("unsupported ranking.scheduler_mode: {}", other),
    };

    let options = RankingServiceOptions {
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

    let service = Arc::new(
        RankingService::new(&config.easyscholar.keys, None, options)
            .expect("ranking service should initialize from config"),
    );

    let all_venues: Vec<String> = (0..300)
        .map(|idx| format!("Venue-{}-{}", idx + 1, Uuid::new_v4()))
        .collect();
    assert_eq!(all_venues.len(), 300);

    let sizes = [160_usize, 20, 50, 30, 40];
    let mut cursor = 0_usize;
    let mut jobs = Vec::new();
    for (job_idx, size) in sizes.iter().enumerate() {
        let end = cursor + size;
        let venues = all_venues[cursor..end].to_vec();
        cursor = end;
        jobs.push((job_idx + 1, *size, venues));
    }
    assert_eq!(cursor, 300);

    let mut handles = Vec::new();
    for (job_no, expected_size, venues) in jobs {
        let service = Arc::clone(&service);
        let handle = tokio::spawn(async move {
            let request = RankingBatchRequest {
                task_id: format!("ranking-live-300-job-{}", job_no),
                venues,
            };
            let result = service
                .lookup_batch(request)
                .await
                .expect("ranking lookup should succeed");
            (job_no, expected_size, result)
        });
        handles.push(handle);
    }

    for handle in handles {
        let (job_no, expected_size, result) = handle.await.expect("join handle should succeed");
        println!(
            "job={} expected={} venue_total={} cache_hits={} cache_misses={} api_hits={} chunk_size={}",
            job_no,
            expected_size,
            result.venue_total,
            result.cache_hits,
            result.cache_misses,
            result.api_hits,
            result.chunk_size_granted
        );
        assert_eq!(
            result.venue_total, expected_size,
            "job {} venue_total mismatch",
            job_no
        );
    }
}
