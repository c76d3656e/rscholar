use crate::db::{journal_cache, DbPool};
use crate::error::{GscholarError, Result};
use async_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, MissedTickBehavior};
use tracing::{debug, info, warn};

use super::{KeyHealthPolicy, RankingClientPool, RankingMetrics};

/// Dynamic key allocation policy for each ranking job.
#[derive(Debug, Clone, Copy)]
pub struct LeasePolicy {
    pub min_chunk: usize,
    pub max_chunk: usize,
}

impl Default for LeasePolicy {
    fn default() -> Self {
        Self {
            min_chunk: 1,
            max_chunk: 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerMode {
    EasyBackfill,
}

/// Runtime options for the in-process ranking service.
#[derive(Debug, Clone, Copy)]
pub struct RankingServiceOptions {
    pub queue_capacity: usize,
    pub lease_policy: LeasePolicy,
    pub max_concurrent_jobs: usize,
    pub scheduler_mode: SchedulerMode,
    pub target_duration_sec: u64,
    pub eta_scale: f64,
    pub heartbeat_ms: u64,
    pub job_timeout_min_sec: u64,
    pub key_health_policy: KeyHealthPolicy,
}

impl Default for RankingServiceOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 128,
            lease_policy: LeasePolicy::default(),
            max_concurrent_jobs: 16,
            scheduler_mode: SchedulerMode::EasyBackfill,
            target_duration_sec: 10,
            eta_scale: 1.6,
            heartbeat_ms: 200,
            job_timeout_min_sec: 15,
            key_health_policy: KeyHealthPolicy::default(),
        }
    }
}

/// Input payload submitted by pipeline.
#[derive(Debug, Clone)]
pub struct RankingBatchRequest {
    pub task_id: String,
    pub venues: Vec<String>,
}

/// Ranking lookup outcome returned to pipeline.
#[derive(Debug, Clone, Default)]
pub struct RankingBatchResult {
    pub by_venue: HashMap<String, RankingMetrics>,
    pub venue_total: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub api_hits: usize,
    pub chunk_size_granted: usize,
}

struct RankingJob {
    request: RankingBatchRequest,
    responder: oneshot::Sender<Result<RankingBatchResult>>,
}

struct PreparedJob {
    task_id: String,
    venue_total: usize,
    cache_hits: usize,
    cache_miss_count: usize,
    cached_rankings: HashMap<String, RankingMetrics>,
    uncached_venues: Vec<String>,
    requested_chunk: usize,
    scan_miss_count: usize,
    enqueued_at: Instant,
    responder: oneshot::Sender<Result<RankingBatchResult>>,
}

struct RunningLease {
    lease_id: u64,
    task_id: String,
    granted_keys: usize,
    eta_finish_at: Instant,
    timeout_at: Instant,
}

struct CompletedJob {
    lease_id: u64,
    job: PreparedJob,
    granted_keys: usize,
    result: Result<RankingBatchResult>,
    timed_out: bool,
}

/// In-process ranking microservice with EASY backfilling scheduler.
///
/// Key pool is created once at startup and reused across all jobs.
#[derive(Clone)]
pub struct RankingService {
    tx: Sender<RankingJob>,
}

impl RankingService {
    /// Create service and start background scheduler loop.
    pub fn new(
        api_keys: &[String],
        db: Option<DbPool>,
        options: RankingServiceOptions,
    ) -> Result<Self> {
        if api_keys.is_empty() {
            return Err(GscholarError::Config(
                "Cannot create RankingService without easyscholar.keys".to_string(),
            ));
        }

        let pool = Arc::new(RankingClientPool::from_api_keys_with_policy(
            api_keys,
            options.key_health_policy,
        )?);
        let (tx, rx) = async_channel::bounded(options.queue_capacity);
        let scheduler = Scheduler {
            rx,
            pool,
            db,
            options,
        };

        tokio::spawn(async move {
            scheduler.run_loop().await;
        });

        Ok(Self { tx })
    }

    /// Submit one batch job and wait for completion.
    pub async fn lookup_batch(&self, req: RankingBatchRequest) -> Result<RankingBatchResult> {
        let (result_tx, result_rx) = oneshot::channel();
        let job = RankingJob {
            request: req,
            responder: result_tx,
        };

        self.tx.send(job).await.map_err(|_| {
            GscholarError::Config("RankingService queue is closed unexpectedly".to_string())
        })?;

        result_rx.await.map_err(|_| {
            GscholarError::Config("RankingService worker dropped response channel".to_string())
        })?
    }

    /// Number of queued jobs waiting for execution.
    pub fn queued_jobs(&self) -> usize {
        self.tx.len()
    }
}

struct Scheduler {
    rx: Receiver<RankingJob>,
    pool: Arc<RankingClientPool>,
    db: Option<DbPool>,
    options: RankingServiceOptions,
}

impl Scheduler {
    async fn run_loop(self) {
        let mut pending_jobs: VecDeque<PreparedJob> = VecDeque::new();
        let mut running_leases: HashMap<u64, RunningLease> = HashMap::new();
        let mut next_lease_id: u64 = 1;
        let mut input_closed = false;
        let (done_tx, mut done_rx) = mpsc::unbounded_channel::<CompletedJob>();
        let mut hb = interval(Duration::from_millis(self.options.heartbeat_ms.max(50)));
        hb.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                recv = self.rx.recv(), if !input_closed => {
                    match recv {
                        Ok(job) => {
                            if job.responder.is_closed() {
                                info!(
                                    task_id = %job.request.task_id,
                                    "Ranking job dropped by caller while queued; skip execution"
                                );
                            } else {
                                match self.prepare_job(job).await {
                                    Some(prepared) => pending_jobs.push_back(prepared),
                                    None => {}
                                }
                            }
                        }
                        Err(_) => {
                            input_closed = true;
                        }
                    }
                }
                maybe_done = done_rx.recv(), if !running_leases.is_empty() => {
                    if let Some(completed) = maybe_done {
                        if running_leases.remove(&completed.lease_id).is_none() {
                            warn!(
                                lease_id = completed.lease_id,
                                task_id = %completed.job.task_id,
                                "Received stale completion for unknown lease"
                            );
                            continue;
                        }

                        if completed.timed_out {
                            warn!(
                                task_id = %completed.job.task_id,
                                lease_id = completed.lease_id,
                                granted_keys = completed.granted_keys,
                                "Ranking lease timed out and has been reclaimed"
                            );
                        }

                        if completed.job.responder.send(completed.result).is_err() {
                            info!(task_id = %completed.job.task_id, "Ranking job completed but caller is no longer waiting");
                        }
                    }
                }
                _ = hb.tick() => {
                    self.cleanup_stale_leases(&mut running_leases);
                }
            }

            self.try_dispatch(
                &mut pending_jobs,
                &mut running_leases,
                &mut next_lease_id,
                &done_tx,
            )
            .await;

            if input_closed && running_leases.is_empty() && pending_jobs.is_empty() {
                break;
            }
        }
    }

    fn cleanup_stale_leases(&self, running_leases: &mut HashMap<u64, RunningLease>) {
        let now = Instant::now();
        let stale: Vec<u64> = running_leases
            .iter()
            .filter_map(|(lease_id, lease)| {
                if now > lease.timeout_at + Duration::from_secs(2) {
                    Some(*lease_id)
                } else {
                    None
                }
            })
            .collect();
        for lease_id in stale {
            if let Some(lease) = running_leases.remove(&lease_id) {
                warn!(
                    task_id = %lease.task_id,
                    lease_id = lease.lease_id,
                    "Lease removed as stale after timeout grace period"
                );
            }
        }
    }

    async fn prepare_job(&self, job: RankingJob) -> Option<PreparedJob> {
        let unique_venues = dedup_venues(&job.request.venues);
        if unique_venues.is_empty() {
            let _ = job.responder.send(Ok(RankingBatchResult {
                venue_total: 0,
                ..Default::default()
            }));
            return None;
        }

        let (cached_rankings, uncached_venues) = self.lookup_cache(&job.request.task_id, &unique_venues).await;
        let cache_misses = uncached_venues.len();

        if cache_misses == 0 {
            let _ = job.responder.send(Ok(RankingBatchResult {
                by_venue: cached_rankings,
                venue_total: unique_venues.len(),
                cache_hits: unique_venues.len(),
                cache_misses: 0,
                api_hits: 0,
                chunk_size_granted: 0,
            }));
            return None;
        }

        let active_keys = self.pool.active_key_count().await.max(1);
        let requested_chunk = compute_requested_chunk(
            cache_misses,
            active_keys,
            self.options.lease_policy,
            self.options.target_duration_sec,
        );

        Some(PreparedJob {
            task_id: job.request.task_id,
            venue_total: unique_venues.len(),
            cache_hits: cached_rankings.len(),
            cache_miss_count: cache_misses,
            cached_rankings,
            uncached_venues,
            requested_chunk,
            scan_miss_count: 0,
            enqueued_at: Instant::now(),
            responder: job.responder,
        })
    }

    async fn try_dispatch(
        &self,
        pending_jobs: &mut VecDeque<PreparedJob>,
        running_leases: &mut HashMap<u64, RunningLease>,
        next_lease_id: &mut u64,
        done_tx: &mpsc::UnboundedSender<CompletedJob>,
    ) {
        if pending_jobs.is_empty() {
            return;
        }

        loop {
            if running_leases.len() >= self.options.max_concurrent_jobs.max(1) {
                break;
            }

            let active_keys = self.pool.active_key_count().await;
            let leased_keys = running_leases.values().map(|l| l.granted_keys).sum::<usize>();
            let available_keys = active_keys.saturating_sub(leased_keys);

            if available_keys == 0 {
                break;
            }

            for job in pending_jobs.iter_mut() {
                job.requested_chunk = compute_requested_chunk(
                    job.cache_miss_count,
                    active_keys.max(1),
                    self.options.lease_policy,
                    self.options.target_duration_sec,
                );
            }

            if pending_jobs.is_empty() {
                break;
            }

            if pending_jobs[0].requested_chunk <= available_keys {
                if let Some(job) = pending_jobs.pop_front() {
                    self.dispatch_job(
                        job,
                        available_keys,
                        "head_exact",
                        running_leases,
                        next_lease_id,
                        done_tx,
                    );
                    continue;
                }
            }

            let reservation = estimate_head_reservation_at(
                pending_jobs[0].requested_chunk,
                available_keys,
                running_leases.values(),
            );
            pending_jobs[0].scan_miss_count += 1;

            if pending_jobs[0].scan_miss_count >= 3 && available_keys > 0 {
                if let Some(job) = pending_jobs.pop_front() {
                    self.dispatch_job(
                        job,
                        available_keys,
                        "head_degraded",
                        running_leases,
                        next_lease_id,
                        done_tx,
                    );
                    continue;
                }
            }

            let mut picked_idx: Option<usize> = None;
            for idx in 1..pending_jobs.len() {
                let candidate = &pending_jobs[idx];
                if candidate.requested_chunk > available_keys {
                    continue;
                }

                let candidate_eta = estimate_runtime_secs(
                    candidate.cache_miss_count,
                    candidate.requested_chunk,
                    self.options.eta_scale,
                );
                let can_backfill = match reservation {
                    Some(t_reserve) => Instant::now() + Duration::from_secs(candidate_eta) <= t_reserve,
                    None => true,
                };
                if can_backfill {
                    picked_idx = Some(idx);
                    break;
                }
            }

            if let Some(idx) = picked_idx {
                if let Some(job) = pending_jobs.remove(idx) {
                    self.dispatch_job(
                        job,
                        available_keys,
                        "backfill",
                        running_leases,
                        next_lease_id,
                        done_tx,
                    );
                    continue;
                }
            }

            debug!(
                queue_len = pending_jobs.len(),
                head_task_id = %pending_jobs[0].task_id,
                head_requested = pending_jobs[0].requested_chunk,
                head_scan_miss = pending_jobs[0].scan_miss_count,
                active_keys = active_keys,
                leased_keys = leased_keys,
                available_keys = available_keys,
                "No dispatchable ranking job in this scheduler tick"
            );
            break;
        }
    }

    fn dispatch_job(
        &self,
        job: PreparedJob,
        available_keys: usize,
        dispatch_mode: &'static str,
        running_leases: &mut HashMap<u64, RunningLease>,
        next_lease_id: &mut u64,
        done_tx: &mpsc::UnboundedSender<CompletedJob>,
    ) {
        let granted_keys = match dispatch_mode {
            "head_degraded" => available_keys.max(1),
            _ => job.requested_chunk.min(available_keys).max(1),
        };

        let runtime_secs = estimate_runtime_secs(job.cache_miss_count, granted_keys, self.options.eta_scale);
        let timeout_secs = std::cmp::max(self.options.job_timeout_min_sec, runtime_secs);
        let lease_id = *next_lease_id;
        *next_lease_id += 1;
        let now = Instant::now();

        running_leases.insert(
            lease_id,
            RunningLease {
                lease_id,
                task_id: job.task_id.clone(),
                granted_keys,
                eta_finish_at: now + Duration::from_secs(runtime_secs),
                timeout_at: now + Duration::from_secs(timeout_secs),
            },
        );

        info!(
            task_id = %job.task_id,
            lease_id = lease_id,
            dispatch_mode = dispatch_mode,
            n = job.cache_miss_count,
            requested_chunk = job.requested_chunk,
            granted_keys = granted_keys,
            eta_secs = runtime_secs,
            timeout_secs = timeout_secs,
            queue_wait_ms = job.enqueued_at.elapsed().as_millis() as u64,
            "Ranking job dispatched"
        );

        let pool = Arc::clone(&self.pool);
        let db = self.db.clone();
        let done_tx = done_tx.clone();
        tokio::spawn(async move {
            let timeout = Duration::from_secs(timeout_secs);
            let mut timed_out = false;
            let result = match tokio::time::timeout(timeout, process_prepared_job(db, pool, granted_keys, &job)).await {
                Ok(result) => result,
                Err(_) => {
                    timed_out = true;
                    Err(GscholarError::Api {
                        code: 504,
                        message: format!("Ranking job timed out after {}s", timeout_secs),
                    })
                }
            };
            let _ = done_tx.send(CompletedJob {
                lease_id,
                job,
                granted_keys,
                result,
                timed_out,
            });
        });
    }

    async fn lookup_cache(
        &self,
        task_id: &str,
        unique_venues: &[String],
    ) -> (HashMap<String, RankingMetrics>, Vec<String>) {
        let mut cached_rankings: HashMap<String, RankingMetrics> = HashMap::new();
        let mut uncached_venues: Vec<String> = Vec::new();

        if let Some(db_pool) = &self.db {
            let venues_for_cache = unique_venues.to_vec();
            match db_pool.get().await {
                Ok(conn) => {
                    let cache_result = conn
                        .interact(move |conn| journal_cache::batch_get(conn, &venues_for_cache))
                        .await;
                    match cache_result {
                        Ok(Ok(cache_map)) => {
                            for venue in unique_venues {
                                if let Some(ranking) = cache_map.get(venue) {
                                    cached_rankings.insert(
                                        venue.clone(),
                                        RankingMetrics {
                                            sciif: ranking.sciif.clone(),
                                            jci: ranking.jci.clone(),
                                            sci: ranking.sci.clone(),
                                            sci_up_top: ranking.sci_up_top.clone(),
                                            sci_base: ranking.sci_base.clone(),
                                            sci_up: ranking.sci_up.clone(),
                                        },
                                    );
                                } else {
                                    uncached_venues.push(venue.clone());
                                }
                            }
                            info!(
                                task_id = %task_id,
                                cached = cached_rankings.len(),
                                uncached = uncached_venues.len(),
                                "Ranking cache lookup complete"
                            );
                        }
                        Ok(Err(e)) => {
                            warn!(task_id = %task_id, error = %e, "Ranking cache query failed; fallback to API");
                            uncached_venues = unique_venues.to_vec();
                        }
                        Err(e) => {
                            warn!(task_id = %task_id, error = %e, "Ranking cache interact failed; fallback to API");
                            uncached_venues = unique_venues.to_vec();
                        }
                    }
                }
                Err(e) => {
                    warn!(task_id = %task_id, error = %e, "Ranking DB pool get failed; fallback to API");
                    uncached_venues = unique_venues.to_vec();
                }
            }
        } else {
            uncached_venues = unique_venues.to_vec();
        }

        (cached_rankings, uncached_venues)
    }
}

async fn process_prepared_job(
    db: Option<DbPool>,
    pool: Arc<RankingClientPool>,
    granted_keys: usize,
    prepared: &PreparedJob,
) -> Result<RankingBatchResult> {
    let mut by_venue = prepared.cached_rankings.clone();
    let api_results = pool
        .batch_lookup_with_chunk(&prepared.uncached_venues, granted_keys)
        .await;

    let api_hits = merge_api_results(
        db,
        &prepared.task_id,
        &prepared.uncached_venues,
        &api_results,
        &mut by_venue,
    )
    .await;

    info!(
        task_id = %prepared.task_id,
        venue_total = prepared.venue_total,
        cache_hits = prepared.cache_hits,
        cache_misses = prepared.cache_miss_count,
        api_hits,
        chunk_size_granted = granted_keys,
        "Ranking job completed"
    );

    Ok(RankingBatchResult {
        by_venue,
        venue_total: prepared.venue_total,
        cache_hits: prepared.cache_hits,
        cache_misses: prepared.cache_miss_count,
        api_hits,
        chunk_size_granted: granted_keys,
    })
}

async fn merge_api_results(
    db: Option<DbPool>,
    task_id: &str,
    uncached_venues: &[String],
    api_results: &[Option<RankingMetrics>],
    by_venue: &mut HashMap<String, RankingMetrics>,
) -> usize {
    let mut api_hits = 0usize;
    let mut cache_entries = Vec::new();

    for (venue, ranking_opt) in uncached_venues.iter().zip(api_results.iter()) {
        if let Some(ranking) = ranking_opt {
            api_hits += 1;
            by_venue.insert(venue.clone(), ranking.clone());

            cache_entries.push(journal_cache::JournalRanking {
                name: venue.clone(),
                sciif: ranking.sciif.clone(),
                jci: ranking.jci.clone(),
                sci: ranking.sci.clone(),
                sci_up_top: ranking.sci_up_top.clone(),
                sci_base: ranking.sci_base.clone(),
                sci_up: ranking.sci_up.clone(),
                fetched_at: chrono::Utc::now().timestamp(),
            });
        }
    }

    if !cache_entries.is_empty() {
        if let Some(db_pool) = &db {
            match db_pool.get().await {
                Ok(conn) => {
                    let entries_len = cache_entries.len();
                    match conn
                        .interact(move |conn| journal_cache::batch_upsert(conn, &cache_entries))
                        .await
                    {
                        Ok(Ok(())) => {
                            info!(
                                task_id = %task_id,
                                entries = entries_len,
                                "Ranking cache batch upsert complete"
                            );
                        }
                        Ok(Err(error)) => {
                            warn!(
                                task_id = %task_id,
                                error = %error,
                                "Ranking cache batch upsert failed"
                            );
                        }
                        Err(error) => {
                            warn!(
                                task_id = %task_id,
                                error = %error,
                                "Ranking cache batch upsert interact failed"
                            );
                        }
                    }
                }
                Err(error) => {
                    warn!(
                        task_id = %task_id,
                        error = %error,
                        "Ranking DB pool get failed on cache batch write"
                    );
                }
            }
        }
    }

    api_hits
}

fn dedup_venues(venues: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    venues
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .filter(|v| seen.insert((*v).to_string()))
        .map(ToString::to_string)
        .collect()
}

fn compute_requested_chunk(
    cache_miss_count: usize,
    total_active_keys: usize,
    policy: LeasePolicy,
    target_duration_sec: u64,
) -> usize {
    if cache_miss_count == 0 || total_active_keys == 0 {
        return 0;
    }

    let target = target_duration_sec.max(1) as usize;
    let mut chunk = cache_miss_count.div_ceil(target);
    chunk = chunk.clamp(policy.min_chunk.max(1), policy.max_chunk.max(1));
    chunk.min(total_active_keys)
}

fn estimate_runtime_secs(cache_miss_count: usize, granted_keys: usize, eta_scale: f64) -> u64 {
    if cache_miss_count == 0 || granted_keys == 0 {
        return 1;
    }
    let base = (cache_miss_count as f64) / (granted_keys as f64);
    (base * eta_scale.max(1.0)).ceil() as u64
}

fn estimate_head_reservation_at<'a>(
    head_requested_chunk: usize,
    available_keys: usize,
    running_leases: impl Iterator<Item = &'a RunningLease>,
) -> Option<Instant> {
    if head_requested_chunk <= available_keys {
        return Some(Instant::now());
    }

    let mut events: Vec<(Instant, usize)> = running_leases
        .map(|lease| (lease.eta_finish_at, lease.granted_keys))
        .collect();
    events.sort_by_key(|(finish, _)| *finish);

    let mut free = available_keys;
    for (finish, released) in events {
        free += released;
        if free >= head_requested_chunk {
            return Some(finish);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        compute_requested_chunk, estimate_head_reservation_at, estimate_runtime_secs, LeasePolicy,
        RunningLease,
    };
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[test]
    fn test_dynamic_chunk_formula_uses_target_duration() {
        let policy = LeasePolicy {
            min_chunk: 1,
            max_chunk: 16,
        };
        assert_eq!(compute_requested_chunk(0, 8, policy, 10), 0);
        assert_eq!(compute_requested_chunk(5, 8, policy, 10), 1);
        assert_eq!(compute_requested_chunk(20, 8, policy, 10), 2);
        assert_eq!(compute_requested_chunk(400, 64, policy, 10), 16);
    }

    #[test]
    fn test_eta_scaled_estimation() {
        let eta = estimate_runtime_secs(20, 4, 1.6);
        assert_eq!(eta, 8);
    }

    #[test]
    fn test_eta_scaled_estimation_fractional_rounds_up() {
        // base = 5/2 = 2.5, eta = ceil(2.5 * 1.6) = ceil(4.0) = 4
        let eta = estimate_runtime_secs(5, 2, 1.6);
        assert_eq!(eta, 4);
    }

    #[test]
    fn test_reservation_is_now_when_head_fits_available_keys() {
        let reservation = estimate_head_reservation_at(3, 3, [].iter());
        let reservation = reservation.expect("reservation should exist");
        assert!(reservation <= Instant::now() + Duration::from_millis(10));
    }

    #[test]
    fn test_reservation_waits_for_running_lease_release() {
        let now = Instant::now();
        let lease = RunningLease {
            lease_id: 1,
            task_id: "head".to_string(),
            granted_keys: 2,
            eta_finish_at: now + Duration::from_secs(3),
            timeout_at: now + Duration::from_secs(10),
        };
        let mut map = HashMap::new();
        map.insert(1_u64, lease);

        let reservation = estimate_head_reservation_at(4, 2, map.values());
        let reservation = reservation.expect("reservation should exist");
        assert!(reservation >= now + Duration::from_secs(3));
    }

    #[test]
    fn test_reservation_none_when_total_future_keys_still_insufficient() {
        let now = Instant::now();
        let lease = RunningLease {
            lease_id: 2,
            task_id: "head2".to_string(),
            granted_keys: 1,
            eta_finish_at: now + Duration::from_secs(2),
            timeout_at: now + Duration::from_secs(10),
        };
        let mut map = HashMap::new();
        map.insert(2_u64, lease);

        // available=1, future release=1 => total 2, requested=3 => impossible
        let reservation = estimate_head_reservation_at(3, 1, map.values());
        assert!(reservation.is_none());
    }
}
