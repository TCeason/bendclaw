# Task/Cron Scheduling: Cross-Project Comparison & Improvement Suggestions for bendclaw

## 1. Feature Comparison Matrix

| Feature | bendclaw | ironclaw | moltis | openclaw | nanobot | Clawith | zeptoclaw |
|---|---|---|---|---|---|---|---|
| **Retry/backoff on failure** | ❌ None | ❌ None (but tracks `consecutive_failures`) | ❌ None | ✅ Exponential backoff (30s→1m→5m→15m→60m) | ❌ None | ❌ None | ❌ None (provider-level retry exists) |
| **Consecutive failure tracking** | ❌ None | ✅ `consecutive_failures` field on Routine | ❌ None (tracks `last_error` only) | ✅ `consecutiveErrors` field, resets on success | ❌ None | ✅ `fire_count` + `max_fires` | ❌ None |
| **Failure alerts/notifications** | ❌ None | ✅ `NotifyConfig` (on_failure, on_success, on_attention) | ❌ None | ✅ `failureAlert` per job (after N errors, cooldown, channel routing) | ❌ None | ❌ None | ❌ None |
| **Concurrency guardrails** | ❌ No per-task limit (lease prevents double-run) | ✅ `max_concurrent` per routine + global `max_concurrent_routines` | ✅ `running_at_ms` prevents double-run | ✅ `maxConcurrentRuns` config + per-job `runningAtMs` | ❌ None | ✅ `DEDUP_WINDOW` (30s per agent) | ✅ `max_concurrent` in RoutinesConfig |
| **Cooldown between runs** | ❌ None | ✅ `cooldown` per routine (default 300s) | ❌ None | ✅ `MIN_REFIRE_GAP_MS` (2s safety net) | ❌ None | ✅ `cooldown_seconds` per trigger | ❌ None |
| **Active hours / quiet hours** | ❌ None | ✅ `quiet_hours_start/end` + timezone on heartbeat | ✅ `is_within_active_hours()` with timezone | ❌ None | ❌ None | ❌ None (but has `expires_at`) | ❌ None |
| **Timezone support** | ✅ `tz` field on Cron schedule (added recently) | ✅ Per-trigger timezone | ✅ Per-job timezone | ✅ Per-job timezone (uses croner) | ✅ Per-job timezone (croniter) | ✅ Per-trigger timezone (croniter) | ✅ Agent-level timezone |
| **Cron auto-padding (5→7 field)** | ❌ None (requires 6/7-field `cron` crate format) | ❌ None (requires cron crate format) | ✅ Auto-pads 5-field to 7-field (`"0 {expr} *"`) | ✅ Uses croner (natively handles 5-field) | ✅ Uses croniter (natively handles 5-field) | ✅ Uses croniter (natively handles 5-field) | ❌ None (uses cron crate) |
| **One-shot cleanup** | ✅ `delete_after_run` for `At` tasks | ✅ N/A (manual trigger type) | ✅ `delete_after_run` or auto-disable | ✅ `deleteAfterRun` (only on success) + auto-disable | ✅ `delete_after_run` or auto-disable | ✅ `once` type auto-disables + `max_fires` | ❌ N/A |
| **Task history / run logging** | ✅ `task_history` table (insert per run) | ✅ `routine_runs` table (per run) | ✅ `CronRunRecord` stored per run (with tokens) | ✅ Run log with telemetry (model, tokens, duration) | ❌ State only (`last_status`, no history) | ✅ Audit log entries | ❌ None |
| **Stuck job detection** | ✅ Lease expiry resets stuck tasks | ✅ N/A (concurrent check) | ✅ `STUCK_THRESHOLD_MS` (2h), auto-clears | ✅ Maintenance recompute + `runningAtMs` tracking | ❌ None | ❌ None | ❌ None |
| **Schedule drift correction** | ❌ None | ❌ None | ❌ None | ✅ `MAX_TIMER_DELAY_MS` (60s) caps timer to poll frequently | ❌ None | ❌ None | ❌ None |
| **Missed run catch-up on restart** | ❌ None (next_run_at recalculated) | ❌ None | ❌ None | ✅ `runMissedJobs()` with stagger + cap | ❌ None | ❌ None | ✅ `on_miss` config (skip/run) |
| **Event-driven triggers** | ❌ Only schedule-based | ✅ Event, SystemEvent, Manual triggers | ❌ SystemEvent injection | ❌ SystemEvent injection | ❌ None | ✅ cron, once, interval, poll, on_message, webhook | ❌ None |
| **Rate limiting on job creation** | ❌ None | ❌ None | ✅ Sliding-window rate limiter | ❌ None | ❌ None | ✅ `WEBHOOK_RATE_LIMIT` (5/min) | ❌ None |
| **Cron description (human-readable)** | ❌ None | ✅ `describe_cron()` with pattern matching | ❌ None | ❌ None | ❌ None | ❌ None | ❌ None |
| **Content-hash dedup (events)** | ❌ N/A | ✅ `dedup_window` + `content_hash()` | ❌ N/A | ❌ N/A | ❌ N/A | ❌ N/A | ❌ N/A |
| **Execution modes** | Single (prompt→LLM) | Lightweight (single LLM) + FullJob (multi-turn) | SystemEvent + AgentTurn | SystemEvent (main) + AgentTurn (isolated) | AgentTurn only | Full LLM w/ tools (50 rounds) | Single agent run |
| **Token usage tracking** | ❌ None | ✅ `tokens_used` on run | ✅ `input_tokens`/`output_tokens` per run | ✅ Full usage summary (input/output/cache) | ❌ None | ❌ None | ❌ None |

## 2. Detailed Gap Analysis & Actionable Improvements

### 2.1 🔴 Retry/Backoff on Task Failure (HIGH PRIORITY)

**What bendclaw lacks:** When a task fails (LLM error, timeout, etc.), bendclaw records the error and moves to the next scheduled run. There's no retry mechanism.

**Best reference:** openclaw `src/cron/service/timer.ts` lines ~85-110
- Exponential backoff schedule: `[30s, 60s, 5min, 15min, 60min]`
- For recurring jobs: `nextRunAtMs = max(normalNext, endedAt + backoff)`
- For one-shot jobs: retries up to 3 times on transient errors, then disables
- Transient error detection via regex patterns (rate_limit, overloaded, network, timeout, server_error)

**Suggested implementation for bendclaw:**
- Add `consecutive_errors: i32` to `TaskRecord`
- In `finish_execution()`, increment on error, reset to 0 on success
- When status is "error", compute `next_run_at = max(schedule.next_run_at(), now + backoff(consecutive_errors))`
- Backoff schedule: `[30s, 60s, 300s, 900s, 3600s]`
- For `At` tasks: retry up to 3 times on transient errors, then disable

**Files to modify:**
- `src/storage/dal/task/record.rs` — add `consecutive_errors` field
- `src/kernel/task/execution.rs` — backoff logic in `finish_execution()`
- `src/storage/dal/task/schedule.rs` — add `next_run_at_with_backoff()` method
- Migration: add `consecutive_errors INT DEFAULT 0` column

### 2.2 🔴 Concurrency Guardrails (HIGH PRIORITY)

**What bendclaw lacks:** No per-task concurrency limit or global max-parallel-tasks cap. The lease system prevents the *same* task from running twice, but there's no cap on total concurrent tasks.

**Best reference:** ironclaw `src/agent/routine.rs` lines ~260-270
```rust
pub struct RoutineGuardrails {
    pub cooldown: Duration,        // min time between fires
    pub max_concurrent: u32,       // max simultaneous runs
    pub dedup_window: Option<Duration>,
}
```
ironclaw's `routine_engine.rs` also has a global `running_count: AtomicUsize` and `config.max_concurrent_routines`.

**Suggested implementation for bendclaw:**
- Add `max_concurrent_tasks` to server config (default: 10)
- In `TaskLeaseResource::on_acquired()`, check `activity_tracker.active_task_count()` against the limit before spawning
- Add per-task `cooldown_seconds: Option<i32>` to `TaskRecord`, check `now - last_run_at >= cooldown` in claim condition

### 2.3 🟡 Cron Expression Auto-Padding (MEDIUM PRIORITY)

**What bendclaw lacks:** The `cron` crate requires 6 or 7 fields (sec min hour dom month dow [year]). Standard 5-field cron expressions (`0 9 * * *`) fail validation.

**Best reference:** moltis `crates/cron/src/schedule.rs` lines ~38-45
```rust
let schedule: Schedule = expr
    .parse()
    .or_else(|_| {
        let padded = format!("0 {expr} *");
        padded.parse::<Schedule>()
    })
```

**Suggested implementation for bendclaw:**
In `TaskSchedule::validate()` and `next_run_at()`, try parsing as-is, then fallback to `"0 {expr} *"`:
```rust
fn parse_cron_expr(expr: &str) -> Result<Schedule, String> {
    Schedule::from_str(expr)
        .or_else(|_| {
            let padded = format!("0 {} *", expr);
            Schedule::from_str(&padded)
        })
        .map_err(|e| format!("invalid cron expression: {e}"))
}
```

**Files to modify:**
- `src/storage/dal/task/schedule.rs` — update `validate()` and `next_run_at()` methods

### 2.4 🟡 Consecutive Failure Tracking & Alerts (MEDIUM PRIORITY)

**What bendclaw lacks:** No tracking of how many times a task has failed in a row. No alert mechanism when a task is failing repeatedly.

**Best reference:** openclaw `src/cron/service/timer.ts` `applyJobResult()` function
- `consecutiveErrors` incremented on error, reset on success
- `failureAlert` config: fires after N consecutive errors, with cooldown, channel routing
- Alert text: `Cron job "${name}" failed ${count} times\nLast error: ${error}`

**Suggested implementation for bendclaw:**
- Add `consecutive_errors` to `TaskRecord` (see 2.1)
- Add optional `failure_alert_threshold: Option<i32>` to task config
- When `consecutive_errors >= threshold`, deliver error notification via existing delivery mechanism

### 2.5 🟡 Active Hours / Schedule Windows (MEDIUM PRIORITY)

**What bendclaw lacks:** Tasks run at any time. No way to restrict execution to business hours.

**Best references:**
- ironclaw `src/agent/heartbeat.rs` — `quiet_hours_start/end` with timezone
- moltis `crates/cron/src/heartbeat.rs` — `is_within_active_hours(start, end, timezone)` supporting overnight windows

**Suggested implementation for bendclaw:**
- Add optional `active_hours: Option<ActiveHoursConfig>` to `TaskRecord` or schedule
- `ActiveHoursConfig { start: String, end: String, timezone: Option<String> }`
- In `claim_due_tasks()` SQL condition, add time-of-day check, or in `TaskLeaseResource::discover()` filter

### 2.6 🟢 Missed Run Catch-up on Restart (LOW PRIORITY)

**What bendclaw lacks:** When the server restarts, tasks that were due during downtime are simply rescheduled for the next occurrence. No catch-up.

**Best reference:** openclaw `src/cron/service/timer.ts` `runMissedJobs()`
- Detects missed runs by comparing `previousRunAtMs > lastRunAtMs`
- Limits to `DEFAULT_MAX_MISSED_JOBS_PER_RESTART = 5`
- Staggers deferred jobs by `DEFAULT_MISSED_JOB_STAGGER_MS = 5000`

zeptoclaw also has `on_miss: OnMiss` config (`Skip` or `Run`).

**Suggested implementation for bendclaw:**
- On startup, query tasks where `next_run_at < NOW()` and `status = 'idle'`
- Execute up to N missed tasks with staggered delays
- Add config option: `missed_task_policy: "skip" | "run"` (default: skip)

### 2.7 🟢 Schedule Drift Correction (LOW PRIORITY)

**What bendclaw lacks:** No mechanism to detect/correct timer drift.

**Best reference:** openclaw `src/cron/service/timer.ts`
- `MAX_TIMER_DELAY_MS = 60_000` — timer is capped at 60s regardless of actual next run
- This ensures the scheduler re-evaluates frequently, catching drift, clock jumps, and new jobs

bendclaw's lease-based polling (`scan_interval_secs: 30`) partially addresses this, but the 30s scan could be made configurable.

### 2.8 🟢 Token Usage Tracking (LOW PRIORITY)

**What bendclaw lacks:** No tracking of LLM token usage per task run.

**Best references:** moltis `CronRunRecord` and openclaw `CronUsageSummary` both track `input_tokens` and `output_tokens`.

**Suggested implementation:** Add `input_tokens: Option<i32>` and `output_tokens: Option<i32>` to `TaskHistoryRecord`.

### 2.9 🟢 Human-Readable Cron Description (LOW PRIORITY)

**What bendclaw lacks:** No way to display cron expressions in human-readable form in the UI.

**Best reference:** ironclaw `src/agent/routine.rs` `describe_cron()` — converts expressions like `"0 0 9 * * MON-FRI"` to `"Weekdays at 9:00 AM"`.

### 2.10 🟢 One-Shot Retry for Transient Errors (LOW PRIORITY)

**What bendclaw lacks:** `At` tasks that fail are just deleted (if `delete_after_run`) or left as-is. No retry.

**Best reference:** openclaw `src/cron/service/timer.ts` `applyJobResult()`
- Detects transient errors via regex patterns
- Retries up to `DEFAULT_MAX_TRANSIENT_RETRIES = 3` with backoff
- Only disables/deletes after permanent error or max retries exhausted

## 3. Priority Summary

| Priority | Feature | Effort | Impact |
|---|---|---|---|
| 🔴 High | Retry/backoff on task failure | Medium (DB migration + logic) | Prevents permanent task failure on transient errors |
| 🔴 High | Concurrency guardrails | Low (config + check) | Prevents resource exhaustion |
| 🟡 Medium | Cron auto-padding (5→7 field) | Low (2 lines) | Better UX for standard cron expressions |
| 🟡 Medium | Consecutive failure tracking | Medium (DB migration) | Foundation for alerts and backoff |
| 🟡 Medium | Active hours / schedule windows | Medium (new field + filter) | Essential for business-hour tasks |
| 🟢 Low | Missed run catch-up | Medium | Nice-to-have for reliability |
| 🟢 Low | Schedule drift correction | Low | Already partially covered by lease polling |
| 🟢 Low | Token usage tracking | Low (DB column) | Cost visibility |
| 🟢 Low | Human-readable cron | Low | UI improvement |
| 🟢 Low | One-shot transient retry | Medium | Edge case improvement |

## 4. Key Code References

| Project | File | Key Feature |
|---|---|---|
| **ironclaw** | `src/agent/routine.rs:258-270` | `RoutineGuardrails` (cooldown, max_concurrent, dedup_window) |
| **ironclaw** | `src/agent/routine_engine.rs:167-191` | Global running count + cooldown check |
| **ironclaw** | `src/agent/routine_engine.rs:410-418` | `consecutive_failures` tracking |
| **ironclaw** | `src/agent/heartbeat.rs:29-45` | `HeartbeatConfig` (quiet hours, fire_at, timezone) |
| **ironclaw** | `src/agent/routine.rs:370-420` | `describe_cron()` human-readable |
| **moltis** | `crates/cron/src/schedule.rs:36-45` | Cron 5-field auto-padding |
| **moltis** | `crates/cron/src/service.rs:298-306` | Stuck job detection (2h threshold) |
| **moltis** | `crates/cron/src/service.rs:47-54` | Rate limiting config |
| **moltis** | `crates/cron/src/heartbeat.rs:106-131` | Active hours check with timezone |
| **openclaw** | `src/cron/service/timer.ts:85-110` | Exponential backoff schedule |
| **openclaw** | `src/cron/service/timer.ts:113-142` | Transient error detection (regex patterns) |
| **openclaw** | `src/cron/service/timer.ts:189-286` | `applyJobResult()` — full state machine |
| **openclaw** | `src/cron/service/timer.ts:330-365` | Missed job catch-up with stagger |
| **openclaw** | `src/cron/types.ts:60-75` | `CronJobState` (consecutiveErrors, lastFailureAlertAtMs) |
| **Clawith** | `backend/app/services/trigger_daemon.py:57-110` | Multi-trigger-type evaluation (cron/once/interval/poll/webhook) |
| **Clawith** | `backend/app/services/trigger_daemon.py:40-43` | Cooldown + max_fires guardrails |
| **zeptoclaw** | `src/config/types.rs:310-325` | `RoutinesConfig` (max_concurrent, jitter_ms, on_miss) |
