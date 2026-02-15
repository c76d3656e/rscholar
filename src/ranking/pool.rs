use crate::error::{GscholarError, Result};
use futures::stream::{self, StreamExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::client::RankingClient;
use super::types::RankingMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Active,
    Cooldown,
    Dead,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyHealthPolicy {
    pub fail_threshold: usize,
    pub cooldown_secs: u64,
    pub stale_ttl_secs: u64,
}

impl Default for KeyHealthPolicy {
    fn default() -> Self {
        Self {
            fail_threshold: 3,
            cooldown_secs: 30,
            stale_ttl_secs: 180,
        }
    }
}

#[derive(Debug, Clone)]
struct KeyRuntimeState {
    state: KeyState,
    consecutive_failures: usize,
    last_success_at: Option<Instant>,
    cooldown_until: Option<Instant>,
    dead_since: Option<Instant>,
}

impl Default for KeyRuntimeState {
    fn default() -> Self {
        Self {
            state: KeyState::Active,
            consecutive_failures: 0,
            last_success_at: None,
            cooldown_until: None,
            dead_since: None,
        }
    }
}

/// Pool of EasyScholar API clients for parallel requests
pub struct RankingClientPool {
    clients: Vec<Arc<RankingClient>>,
    next_index: AtomicUsize,
    key_states: Arc<Mutex<Vec<KeyRuntimeState>>>,
    health_policy: KeyHealthPolicy,
}

impl RankingClientPool {
    pub fn new(api_keys_csv: &str) -> Result<Self> {
        let keys: Vec<String> = api_keys_csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        Self::from_api_keys(&keys)
    }

    pub fn from_api_keys(keys: &[String]) -> Result<Self> {
        Self::from_api_keys_with_policy(keys, KeyHealthPolicy::default())
    }

    pub fn from_api_keys_with_policy(keys: &[String], health_policy: KeyHealthPolicy) -> Result<Self> {
        if keys.is_empty() {
            return Err(GscholarError::Config("No API keys provided".to_string()));
        }

        let mut clients = Vec::with_capacity(keys.len());
        for key in keys {
            let client = RankingClient::new(key.to_string())?;
            clients.push(Arc::new(client));
        }

        info!(key_count = clients.len(), "Created EasyScholar client pool");
        Ok(Self {
            clients,
            next_index: AtomicUsize::new(0),
            key_states: Arc::new(Mutex::new(vec![KeyRuntimeState::default(); keys.len()])),
            health_policy,
        })
    }

    pub fn key_count(&self) -> usize {
        self.clients.len()
    }

    pub async fn active_key_count(&self) -> usize {
        let mut states = self.key_states.lock().await;
        self.refresh_states(&mut states);
        states.iter().filter(|s| s.state == KeyState::Active).count()
    }

    pub async fn get_rank(&self, venue_name: &str) -> Option<RankingMetrics> {
        let indices = self.pick_key_indices(1).await;
        let idx = indices
            .first()
            .copied()
            .unwrap_or_else(|| self.next_index.fetch_add(1, Ordering::Relaxed) % self.clients.len());
        let (result, ok) = self.clients[idx].get_rank_with_status(venue_name).await;
        self.on_key_result(idx, ok).await;
        result
    }

    pub async fn batch_lookup(&self, venues: &[String]) -> Vec<Option<RankingMetrics>> {
        self.batch_lookup_with_chunk(venues, self.clients.len()).await
    }

    /// Batch lookup using at most `chunk_size` clients from the pool.
    ///
    /// This is used by the ranking scheduler to enforce per-job key chunk leasing.
    pub async fn batch_lookup_with_chunk(
        &self,
        venues: &[String],
        chunk_size: usize,
    ) -> Vec<Option<RankingMetrics>> {
        if venues.is_empty() {
            return Vec::new();
        }

        let picked = self.pick_key_indices(chunk_size).await;
        if picked.is_empty() {
            warn!(
                requested_chunk = chunk_size,
                pool_keys = self.clients.len(),
                "No active key available for ranking lookup"
            );
            return vec![None; venues.len()];
        }

        let num_keys = picked.len();
        let total = venues.len();
        info!(
            venues = total,
            keys = num_keys,
            pool_keys = self.clients.len(),
            "Starting parallel EasyScholar batch lookup"
        );

        let assignments: Vec<(usize, &String, usize)> = venues
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v, i % num_keys))
            .collect();

        let mut key_tasks: Vec<Vec<(usize, &String)>> = vec![Vec::new(); num_keys];
        for (orig_idx, venue, key_idx) in assignments {
            key_tasks[key_idx].push((orig_idx, venue));
        }

        let workers: Vec<_> = key_tasks
            .into_iter()
            .enumerate()
            .map(|(key_idx, tasks)| {
                let real_idx = picked[key_idx];
                let client = Arc::clone(&self.clients[real_idx]);
                let pool = Arc::clone(&self.key_states);
                let policy = self.health_policy;
                let tasks_owned: Vec<(usize, String)> =
                    tasks.into_iter().map(|(idx, v)| (idx, v.clone())).collect();

                tokio::spawn(async move {
                    let mut results = Vec::new();
                    let mut key_ok = true;
                    for (orig_idx, venue) in tasks_owned {
                        let (result, ok) = client.get_rank_with_status(&venue).await;
                        if !ok {
                            key_ok = false;
                        }
                        results.push((orig_idx, result));
                    }
                    let mut states = pool.lock().await;
                    if let Some(state) = states.get_mut(real_idx) {
                        apply_key_result_state(state, key_ok, policy);
                    }
                    results
                })
            })
            .collect();

        let mut all_results: Vec<(usize, Option<RankingMetrics>)> = Vec::with_capacity(total);
        for worker in workers {
            match worker.await {
                Ok(results) => all_results.extend(results),
                Err(e) => warn!(error = %e, "Worker task panicked"),
            }
        }

        all_results.sort_by_key(|(idx, _)| *idx);
        let final_results: Vec<Option<RankingMetrics>> =
            all_results.into_iter().map(|(_, result)| result).collect();

        let matched = final_results.iter().filter(|r| r.is_some()).count();
        info!(total = total, matched = matched, "Parallel batch lookup complete");
        final_results
    }

    async fn pick_key_indices(&self, requested: usize) -> Vec<usize> {
        let mut states = self.key_states.lock().await;
        self.refresh_states(&mut states);

        let active_indices: Vec<usize> = states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state == KeyState::Active)
            .map(|(idx, _)| idx)
            .collect();

        if active_indices.is_empty() {
            return Vec::new();
        }

        let take = requested.clamp(1, active_indices.len());
        let start = self.next_index.fetch_add(1, Ordering::Relaxed) % active_indices.len();
        (0..take)
            .map(|i| active_indices[(start + i) % active_indices.len()])
            .collect()
    }

    async fn on_key_result(&self, idx: usize, ok: bool) {
        let mut states = self.key_states.lock().await;
        if let Some(state) = states.get_mut(idx) {
            apply_key_result_state(state, ok, self.health_policy);
        }
    }

    fn refresh_states(&self, states: &mut [KeyRuntimeState]) {
        let now = Instant::now();
        for state in states.iter_mut() {
            match state.state {
                KeyState::Active => {}
                KeyState::Cooldown => {
                    if let Some(until) = state.cooldown_until {
                        if now >= until {
                            state.state = KeyState::Active;
                            state.cooldown_until = None;
                            debug!("Ranking key recovered from cooldown");
                        }
                    }
                }
                KeyState::Dead => {
                    if let Some(dead_since) = state.dead_since {
                        if now.duration_since(dead_since).as_secs() >= self.health_policy.stale_ttl_secs {
                            state.state = KeyState::Active;
                            state.dead_since = None;
                            state.consecutive_failures = 0;
                            debug!("Ranking key dead TTL expired, probing as active");
                        }
                    }
                }
            }
        }
    }

    pub async fn batch_lookup_cached(
        &self,
        venues: &[String],
        conn: Option<&rusqlite::Connection>,
    ) -> Vec<Option<RankingMetrics>> {
        if venues.is_empty() {
            return Vec::new();
        }

        let total = venues.len();
        let mut results: Vec<Option<RankingMetrics>> = vec![None; total];
        let mut cache_misses: Vec<(usize, String)> = Vec::new();

        if let Some(c) = conn {
            for (idx, venue) in venues.iter().enumerate() {
                if let Ok(Some(cached)) = crate::db::journal_cache::get(c, venue) {
                    results[idx] = Some(RankingMetrics {
                        sciif: cached.sciif,
                        jci: cached.jci,
                        sci: cached.sci,
                        sci_up_top: cached.sci_up_top,
                        sci_base: cached.sci_base,
                        sci_up: cached.sci_up,
                    });
                } else {
                    cache_misses.push((idx, venue.clone()));
                }
            }
        } else {
            cache_misses = venues.iter().enumerate().map(|(i, v)| (i, v.clone())).collect();
        }

        let cache_hits = total - cache_misses.len();
        info!(
            total = total,
            cache_hits = cache_hits,
            cache_misses = cache_misses.len(),
            "EasyScholar cache check complete"
        );

        if !cache_misses.is_empty() {
            let miss_venues: Vec<String> = cache_misses.iter().map(|(_, v)| v.clone()).collect();
            let api_results = self.batch_lookup(&miss_venues).await;

            for ((orig_idx, venue), api_result) in cache_misses.iter().zip(api_results.iter()) {
                results[*orig_idx] = api_result.clone();

                if let Some(c) = conn {
                    let metrics = api_result.as_ref().cloned().unwrap_or_default();
                    let cache_entry = crate::db::journal_cache::JournalRanking {
                        name: venue.clone(),
                        sciif: metrics.sciif,
                        jci: metrics.jci,
                        sci: metrics.sci,
                        sci_up_top: metrics.sci_up_top,
                        sci_base: metrics.sci_base,
                        sci_up: metrics.sci_up,
                        fetched_at: chrono::Utc::now().timestamp(),
                    };
                    let _ = crate::db::journal_cache::upsert(c, &cache_entry);
                }
            }
        }

        let matched = results.iter().filter(|r| r.is_some()).count();
        info!(
            total = total,
            matched = matched,
            cache_hits = cache_hits,
            "Cached batch lookup complete"
        );
        results
    }

    pub async fn batch_lookup_with_progress<F>(
        &self,
        venues: &[String],
        mut on_progress: F,
    ) -> Vec<Option<RankingMetrics>>
    where
        F: FnMut(usize, usize) + Send,
    {
        if venues.is_empty() {
            return Vec::new();
        }

        let total = venues.len();
        let processed = Arc::new(AtomicUsize::new(0));

        stream::iter(venues.iter().enumerate())
            .map(|(i, venue)| {
                let client_idx = i % self.clients.len();
                let client = Arc::clone(&self.clients[client_idx]);
                let venue = venue.clone();
                let processed = Arc::clone(&processed);

                async move {
                    let result = client.get_rank(&venue).await;
                    let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    (done, result)
                }
            })
            .buffer_unordered(self.clients.len())
            .map(|(done, result)| {
                on_progress(done, total);
                result
            })
            .collect()
            .await
    }
}

fn apply_key_result_state(state: &mut KeyRuntimeState, ok: bool, policy: KeyHealthPolicy) {
    if ok {
        state.state = KeyState::Active;
        state.consecutive_failures = 0;
        state.last_success_at = Some(Instant::now());
        state.cooldown_until = None;
        state.dead_since = None;
        return;
    }

    state.consecutive_failures += 1;
    let now = Instant::now();
    if state.consecutive_failures >= policy.fail_threshold {
        state.state = KeyState::Dead;
        state.dead_since = Some(now);
        state.cooldown_until = None;
        warn!(
            failures = state.consecutive_failures,
            stale_ttl_secs = policy.stale_ttl_secs,
            "Ranking key moved to dead state"
        );
    } else {
        state.state = KeyState::Cooldown;
        state.cooldown_until = Some(now + Duration::from_secs(policy.cooldown_secs));
        warn!(
            failures = state.consecutive_failures,
            cooldown_secs = policy.cooldown_secs,
            "Ranking key moved to cooldown state"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_apply_key_result_state_success_resets_failures() {
        let policy = KeyHealthPolicy {
            fail_threshold: 3,
            cooldown_secs: 1,
            stale_ttl_secs: 1,
        };
        let mut state = KeyRuntimeState {
            state: KeyState::Cooldown,
            consecutive_failures: 2,
            last_success_at: None,
            cooldown_until: Some(Instant::now() + Duration::from_secs(5)),
            dead_since: None,
        };
        apply_key_result_state(&mut state, true, policy);
        assert_eq!(state.state, KeyState::Active);
        assert_eq!(state.consecutive_failures, 0);
        assert!(state.last_success_at.is_some());
        assert!(state.cooldown_until.is_none());
        assert!(state.dead_since.is_none());
    }

    #[test]
    fn test_apply_key_result_state_failure_moves_to_cooldown_then_dead() {
        let policy = KeyHealthPolicy {
            fail_threshold: 2,
            cooldown_secs: 5,
            stale_ttl_secs: 30,
        };
        let mut state = KeyRuntimeState::default();
        apply_key_result_state(&mut state, false, policy);
        assert_eq!(state.state, KeyState::Cooldown);
        assert_eq!(state.consecutive_failures, 1);
        assert!(state.cooldown_until.is_some());

        apply_key_result_state(&mut state, false, policy);
        assert_eq!(state.state, KeyState::Dead);
        assert_eq!(state.consecutive_failures, 2);
        assert!(state.dead_since.is_some());
    }

    #[tokio::test]
    async fn test_active_key_count_excludes_cooldown_and_dead() {
        let pool = RankingClientPool::from_api_keys_with_policy(
            &["k1".to_string(), "k2".to_string(), "k3".to_string()],
            KeyHealthPolicy {
                fail_threshold: 2,
                cooldown_secs: 30,
                stale_ttl_secs: 30,
            },
        )
        .expect("pool");

        {
            let mut states = pool.key_states.lock().await;
            states[1].state = KeyState::Cooldown;
            states[1].cooldown_until = Some(Instant::now() + Duration::from_secs(60));
            states[2].state = KeyState::Dead;
            states[2].dead_since = Some(Instant::now());
        }

        let active = pool.active_key_count().await;
        assert_eq!(active, 1);
    }

    #[tokio::test]
    async fn test_refresh_states_recovers_cooldown_and_dead_after_ttl() {
        let policy = KeyHealthPolicy {
            fail_threshold: 3,
            cooldown_secs: 1,
            stale_ttl_secs: 1,
        };
        let pool = RankingClientPool::from_api_keys_with_policy(
            &["k1".to_string(), "k2".to_string()],
            policy,
        )
        .expect("pool");

        {
            let mut states = pool.key_states.lock().await;
            states[0].state = KeyState::Cooldown;
            states[0].cooldown_until = Some(Instant::now() - Duration::from_secs(1));
            states[1].state = KeyState::Dead;
            states[1].dead_since = Some(Instant::now() - Duration::from_secs(2));
            pool.refresh_states(&mut states);
            assert_eq!(states[0].state, KeyState::Active);
            assert_eq!(states[1].state, KeyState::Active);
        }
    }

    #[tokio::test]
    async fn test_pick_key_indices_only_returns_active_keys() {
        let pool = RankingClientPool::from_api_keys_with_policy(
            &["k1".to_string(), "k2".to_string(), "k3".to_string()],
            KeyHealthPolicy::default(),
        )
        .expect("pool");

        {
            let mut states = pool.key_states.lock().await;
            states[0].state = KeyState::Dead;
            states[0].dead_since = Some(Instant::now());
        }

        let picked = pool.pick_key_indices(2).await;
        assert_eq!(picked.len(), 2);
        assert!(!picked.contains(&0));
    }
}
