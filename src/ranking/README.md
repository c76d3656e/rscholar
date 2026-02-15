# Ranking Scheduler Design

This module now uses an in-process scheduler inspired by **Slurm EASY backfilling**.

## Key points

- FIFO queue is preserved.
- Head job is preferred (`head_exact`).
- If head cannot start, scheduler computes a reservation window and allows safe backfill.
- Heartbeat-driven scheduling (`heartbeat_ms`) avoids deadlock when no new events arrive.
- Dynamic key request per job is based on miss size `n`.

## Runtime formula

- `k_target = ceil(n / target_duration_sec)`
- `requested_chunk = clamp(k_target, min_chunk, max_chunk)`
- `t_est = ceil((n / k) * eta_scale)` with default `eta_scale = 1.6`

Where:
- `n` = uncached venues in this job
- `k` = granted keys

## Key state machine

Each key has health state:

- `Active`
- `Cooldown` (temporary)
- `Dead` (recovered after stale TTL)

State transitions:

- request success -> `Active`, reset failure counter
- request failure (below threshold) -> `Cooldown`
- consecutive failures >= threshold -> `Dead`
- `Cooldown` expires -> `Active`
- `Dead` stale TTL expires -> `Active` (probe-again behavior)

Config:

- `key_fail_threshold`
- `key_cooldown_sec`
- `key_stale_ttl_sec`

## Timeout and lease safety

- Running lease timeout is bounded by `max(job_timeout_min_sec, t_est)`.
- Timed-out lease is reclaimed and reported as failure.
- Stale lease cleanup runs in heartbeat loop.

## Config (`[ranking]`)

- `queue_capacity`
- `min_chunk` (now default `1`)
- `max_chunk`
- `max_concurrent_jobs`
- `scheduler_mode = "easy_backfill"`
- `target_duration_sec`
- `eta_scale`
- `heartbeat_ms`
- `job_timeout_min_sec`
- `key_fail_threshold`
- `key_cooldown_sec`
- `key_stale_ttl_sec`

