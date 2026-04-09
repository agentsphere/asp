# Plan: Unified Operations Page

## Context

The platform currently has 4 separate observe pages (`/observe/logs`, `/observe/traces`, `/observe/metrics`, `/observe/alerts`) showing raw telemetry in isolation. The project-level ObserveTab (`ui/src/components/ObserveTab.tsx`, 991 lines) renders a rich unified dashboard — component health, service topology, request load timeline, error breakdown, traces, logs, metrics, and alerts — all in one view. But it's wired to mock data.

The platform wraps its own infrastructure with the proxy via distroless init containers + iptables transparent proxying. The platform binary, Postgres, Valkey, and MinIO all emit telemetry through the OTLP pipeline. Traces, logs, RED metrics, and Postgres/Redis scraper stats already flow into the observe tables. However, **the proxy does not collect process-level CPU and memory metrics** — needed for the resource utilization charts.

**Current proxy state** (post mesh-proxy changes):
- Transparent proxy mode: `PROXY_TRANSPARENT=true`, `PROXY_INBOUND_PORT=15006`, iptables REDIRECT
- Distroless init container `platform-proxy-init:v1` copies binary + sets iptables rules
- mTLS with SPIFFE identity (strict/permissive modes)
- Gateway mode (`--gateway`) for K8s ingress via HTTPRoute CRDs
- 13 proxy modules including gateway/, scraper, inbound/outbound mTLS, TCP proxy
- `process_metrics.rs` does NOT exist yet

**Goal**: Build a new live Operations page at `/observe` powered by real API data for the platform's own infrastructure. Keep the existing mock ObserveTab on projects as a visual reference ("the target") while we iterate on the live version. Once the live page matches the mock's quality, remove the mock and wire projects to the real component too.

## Design Principles

- **Mock as reference, live as new build** — rename existing `ObserveTab` → `ObserveTabMock` (keeps mock data, stays on project pages as the visual target). Build a new `ObserveTab` component wired to real APIs. Once live matches mock, swap projects over and delete the mock.
- **One index migration** — aggregation queries need indexes on `spans(kind, started_at)`, `traces(started_at)`, etc. to avoid full table scans.
- **Add process metrics to proxy** — emit `process.cpu.utilization` and `process.memory.rss` so we get stored time-series for CPU/MEM charts.
- **SLOs deferred** — SLO bar keeps mock data for v1 (needs new table + evaluation engine).
- **Backend aggregations** — 5 new query endpoints for topology, errors, trace aggregation, load timeline, component health.
- **Namespace derived, never user-supplied** — component health resolves namespace from project_id or platform config. No user-supplied namespace param (IDOR risk).
- **Clear metric ownership split** — two independent producers, one shared `metric_samples` sink:
  - **Proxy** (`crates/proxy/src/proxy/process_metrics.rs`): real-time *local utilization* — cgroups CPU/MEM usage, via OTLP. Runs inside each container.
  - **Backend** (`k8s_watcher.rs`): *global cluster state* — CPU/MEM requests+limits, pod readiness, restarts, replicas. Runs once in the platform binary, streams K8s events.
  - Both converge in `metric_samples` → the query endpoints and UI treat them identically.
- **`IS NOT DISTINCT FROM` for project scoping** — the UI sends `project_id=NULL` for "Platform Infrastructure" mode. Platform's own telemetry (proxy, postgres, valkey) is written with `project_id IS NULL`. A naïve `($1::uuid IS NULL OR project_id = $1)` filter becomes a global wildcard when `$1` is NULL, leaking all project data into the platform view. Fix: use `project_id IS NOT DISTINCT FROM $1::uuid` — this treats NULL as a matchable value, so NULL matches only NULL rows and a UUID matches only that UUID. No fake project needed.

## Data Inventory: What We Have vs What We Need

### Already collected and stored (proxy → OTLP → `metric_series`/`metric_samples`):

| Metric | Source | Labels |
|---|---|---|
| `http.server.request.count` | proxy RED | `service` |
| `http.server.error.count` | proxy RED | `service` |
| `http.server.request.duration_sum` | proxy RED | `service` |
| `http.server.request.duration_bucket` | proxy RED | `service`, `bucket` (le_5..le_inf) |
| `postgresql.backends` | postgres scraper | `service` |
| `postgresql.commits`, `.rollbacks` | postgres scraper | `service` |
| `postgresql.rows_*` | postgres scraper | `service` |
| `postgresql.deadlocks`, `.temp_files`, `.temp_bytes` | postgres scraper | `service` |
| `postgresql.db_size` | postgres scraper | `service`, `database` |
| `postgresql.connections` | postgres scraper | `service`, `state` |
| `redis.memory.used/rss/peak/lua` | redis scraper | `service` |
| `redis.clients.connected/blocked` | redis scraper | `service` |
| `redis.commands.processed` | redis scraper | `service` |
| `redis.keyspace.hits/misses` | redis scraper | `service` |
| `redis.keys.expired/evicted` | redis scraper | `service` |
| `redis.uptime` | redis scraper | `service` |

### Already collected (proxy → OTLP → `traces`/`spans`/`log_entries`):

- HTTP server spans with `http.status_code`, `http.method`, `http.url`, `net.peer.ip` attributes
- HTTP client spans with `http.status_code`
- TCP connection spans with `net.bytes_transferred`
- Stdout/stderr logs with trace/span correlation, level, parsed JSON attributes

### NOT collected — needs to be added:

| Metric | Needed For | Solution |
|---|---|---|
| `process.cpu.utilization` (millicores) | CPU chart | Add to proxy: read cgroups v2/v1 (container-scoped) |
| `process.memory.rss` (bytes) | Memory chart | Add to proxy: read cgroups v2/v1 (container-scoped) |

### Key schema note

The `spans` table currently has **no `project_id` column** — PR 0 adds it (along with `session_id` and `user_id`) to match the `log_entries` pattern. After PR 0, all aggregation queries use direct `WHERE project_id = $N` instead of correlated subqueries through `traces`.

---

## PR 0: Migration — Denormalize `spans` + Add Performance Indexes

The `spans` table is the hottest observe table (every HTTP request = 1+ spans) but has no `project_id`, `session_id`, or useful composite indexes. Every project-scoped query forces a correlated subquery: `WHERE trace_id IN (SELECT trace_id FROM traces WHERE project_id = $X)` — this is an O(N) scan against potentially millions of rows.

Compare with `log_entries`, which got this right: it has `project_id`, `session_id` denormalized with proper indexes.

The `SpanRecord` struct already carries `project_id`, `session_id`, and `user_id` — the data is available at write time but not written to the `spans` table.

- [x] Migration applied
- [x] Store layer updated
- [x] Parquet rotation updated
- [x] Existing queries updated
- [x] Tests passing

### Current observe tables inventory

```
traces          — 12 cols, 1 index (UNIQUE trace_id)
spans           — 13 cols, 2 indexes (trace_id, UNIQUE span_id)
log_entries     — 15 cols, 6 indexes (well-indexed, has project_id/session_id)
metric_series   — 8 cols, 1 index (UNIQUE name+labels)
metric_samples  — 3 cols, PK (series_id, timestamp)
alert_rules     — 11 cols, 0 indexes (!)
alert_events    — 7 cols, 1 index (status+created_at)
```

### Migration: `YYYYMMDDHHMMSS_observe_denormalize_spans`

**Up:**
```sql
-- ============================================================
-- 1. Denormalize spans: add project_id, session_id, user_id
--    (mirrors log_entries pattern — disk is cheap, CPU on
--    correlated subqueries against millions of rows is not)
-- ============================================================

ALTER TABLE spans ADD COLUMN project_id UUID REFERENCES projects(id);
ALTER TABLE spans ADD COLUMN session_id UUID REFERENCES agent_sessions(id);
ALTER TABLE spans ADD COLUMN user_id UUID REFERENCES users(id);

-- No backfill needed: pre-alpha, no running installations.
-- New spans will be written with project_id/session_id/user_id
-- from the store layer update below.

-- ============================================================
-- 2. Composite indexes for aggregation queries
-- ============================================================

-- Topology: self-join on spans filtering by kind + time range + project
CREATE INDEX idx_spans_project_kind_started
    ON spans(project_id, kind, started_at);

-- Error breakdown: filter by status=error + kind=server + time
CREATE INDEX idx_spans_status_kind_started
    ON spans(status, kind, started_at);

-- Session timeline: spans for a given session
CREATE INDEX idx_spans_session_started
    ON spans(session_id, started_at)
    WHERE session_id IS NOT NULL;

-- Trace aggregation: filter by project + time range
CREATE INDEX idx_traces_project_started
    ON traces(project_id, started_at);

-- Trace aggregation (global): time range only
CREATE INDEX idx_traces_started
    ON traces(started_at);

-- Deploy markers for load timeline
CREATE INDEX idx_deploy_releases_project_started
    ON deploy_releases(project_id, started_at);

-- Alert rules by project (missing)
CREATE INDEX idx_alert_rules_project
    ON alert_rules(project_id);
```

**Down:**
```sql
ALTER TABLE spans DROP COLUMN IF EXISTS project_id;
ALTER TABLE spans DROP COLUMN IF EXISTS session_id;
ALTER TABLE spans DROP COLUMN IF EXISTS user_id;

DROP INDEX IF EXISTS idx_spans_project_kind_started;
DROP INDEX IF EXISTS idx_spans_status_kind_started;
DROP INDEX IF EXISTS idx_spans_session_started;
DROP INDEX IF EXISTS idx_traces_project_started;
DROP INDEX IF EXISTS idx_traces_started;
DROP INDEX IF EXISTS idx_deploy_releases_project_started;
DROP INDEX IF EXISTS idx_alert_rules_project;
```

### Store layer update: `src/observe/store.rs`

Update `write_spans()` to include the three new columns in the UNNEST INSERT:

```rust
// Before (12 columns):
INSERT INTO spans (trace_id, span_id, parent_span_id, name, service, kind, status,
                   attributes, events, duration_ms, started_at, finished_at)

// After (15 columns):
INSERT INTO spans (trace_id, span_id, parent_span_id, name, service, kind, status,
                   attributes, events, duration_ms, started_at, finished_at,
                   project_id, session_id, user_id)
```

Add three more UNNEST arrays for `project_ids`, `session_ids`, `user_ids` — same pattern as the existing columns.

### Parquet rotation update: `src/observe/parquet.rs`

Update `rotate_spans()` SELECT and `span_schema()` to include the new columns so they're preserved in cold storage.

### Query simplification (PR 2 benefits)

With `project_id` on `spans`, the topology and error queries become direct:

```sql
-- Before (correlated subquery):
WHERE trace_id IN (SELECT trace_id FROM traces WHERE project_id = $3)

-- After (direct filter):
WHERE project_id = $3
```

Similarly, the session timeline query in `query.rs` (`session_timeline` handler) can drop the `JOIN traces t ON t.trace_id = s.trace_id WHERE t.session_id = $1` and use `WHERE s.session_id = $1` directly.

### Code Changes — PR 0

| File | Change |
|---|---|
| `migrations/YYYYMMDDHHMMSS_observe_denormalize_spans.up.sql` | **New** — ALTER TABLE (3 cols) + 7 indexes |
| `migrations/YYYYMMDDHHMMSS_observe_denormalize_spans.down.sql` | **New** — reverse |
| `src/observe/store.rs` | Update `write_spans()` UNNEST to include project_id, session_id, user_id |
| `src/observe/parquet.rs` | Update `rotate_spans()` SELECT + `span_schema()` to include new cols |
| `src/observe/query.rs` | Update `session_timeline` to use `s.session_id` directly instead of join |
| `.sqlx/` | Regenerate offline cache |

### Test Strategy — PR 0

| Test | Validates | Tier |
|---|---|---|
| `spans_written_with_project_id` | `write_spans` stores project_id/session_id on span rows | Integration |
| `session_timeline_uses_span_session` | Session timeline query returns spans via direct session_id filter | Integration |

**Existing tests:** All observe integration tests continue to pass — the new columns are nullable, and the INSERT adds them alongside existing columns. No breaking change.

### Verification
- `just db-migrate` applies cleanly
- `just db-prepare` regenerates offline cache
- `just test-unit` passes
- `just test-integration` passes (existing observe tests still work)

---

## PR 1: Proxy — Add Container-Aware CPU and Memory Metrics

Emit `process.cpu.utilization` and `process.memory.rss` using **cgroup stats** (not `/proc/<pid>/stat`), so the metrics reflect actual container-level usage against K8s limits/requests — not misleading host-level numbers.

- [x] Types & errors defined
- [x] Tests written (red phase)
- [x] Implementation complete (green phase)
- [x] Quality gate passed

> **Deviation:** Proxy code lives in `crates/proxy/`, not `src/proxy/` (which is a stale copy).
> All changes applied to `crates/proxy/src/proxy/process_metrics.rs` and `crates/proxy/src/main.rs`.
> Added `#[allow(clippy::too_many_lines)]` on `main()` and `#[allow(clippy::cast_precision_loss)]` on u64→f64 casts (pedantic clippy in proxy crate).

### Why cgroups, not /proc

In a containerized environment, `/proc/<pid>/stat` reports CPU ticks against the **host node's capacity**, not the container's cgroup limits. A pod with a 500m CPU limit on a 16-core node would show artificially low utilization, and the metric wouldn't reflect cgroup throttling. The cgroup filesystem gives the real container-scoped numbers that correlate with K8s requests/limits.

### Implementation

New file: `crates/proxy/src/proxy/process_metrics.rs`

**Cgroup v2 (modern K8s, default):**
- Memory: `/sys/fs/cgroup/memory.current` — current RSS in bytes
- CPU: `/sys/fs/cgroup/cpu.stat` — parse `usage_usec` line for total CPU microseconds

**Cgroup v1 (legacy K8s):**
- Memory: `/sys/fs/cgroup/memory/memory.usage_in_bytes`
- CPU: `/sys/fs/cgroup/cpu/cpuacct.usage` — total CPU nanoseconds

**Fallback chain:** try v2 → try v1 → return `None` (macOS, non-container).

```rust
use std::time::Instant;

pub struct CgroupSnapshot {
    /// Total CPU time consumed by the container (microseconds).
    pub cpu_usage_usec: u64,
    /// Current memory usage of the container (bytes).
    pub mem_bytes: u64,
    /// Wall-clock time of this reading.
    pub sampled_at: Instant,
}

/// Read container resource usage from cgroup filesystem.
/// Tries cgroup v2 first, falls back to v1, returns None on non-Linux/non-container.
pub fn read_cgroup_stats() -> Option<CgroupSnapshot> {
    // cgroup v2: /sys/fs/cgroup/memory.current + /sys/fs/cgroup/cpu.stat
    if let Some(snap) = read_cgroup_v2() {
        return Some(snap);
    }
    // cgroup v1: /sys/fs/cgroup/memory/memory.usage_in_bytes + cpuacct.usage
    read_cgroup_v1()
}

fn read_cgroup_v2() -> Option<CgroupSnapshot> {
    let mem_bytes: u64 = std::fs::read_to_string("/sys/fs/cgroup/memory.current")
        .ok()?.trim().parse().ok()?;

    let cpu_stat = std::fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()?;
    let cpu_usage_usec: u64 = cpu_stat.lines()
        .find(|l| l.starts_with("usage_usec"))?
        .split_whitespace().nth(1)?
        .parse().ok()?;

    Some(CgroupSnapshot { cpu_usage_usec, mem_bytes, sampled_at: Instant::now() })
}

fn read_cgroup_v1() -> Option<CgroupSnapshot> {
    let mem_bytes: u64 = std::fs::read_to_string(
        "/sys/fs/cgroup/memory/memory.usage_in_bytes"
    ).ok()?.trim().parse().ok()?;

    // cpuacct.usage is in nanoseconds → convert to microseconds
    let cpu_ns: u64 = std::fs::read_to_string(
        "/sys/fs/cgroup/cpu/cpuacct.usage"
    ).ok()?.trim().parse().ok()?;

    Some(CgroupSnapshot {
        cpu_usage_usec: cpu_ns / 1000,
        mem_bytes,
        sampled_at: Instant::now(),
    })
}

/// Compute CPU millicores from delta between two snapshots.
pub fn cpu_millicores(prev: &CgroupSnapshot, curr: &CgroupSnapshot) -> f64 {
    let elapsed = curr.sampled_at.duration_since(prev.sampled_at).as_secs_f64();
    if elapsed <= 0.0 { return 0.0; }
    let delta_usec = curr.cpu_usage_usec.saturating_sub(prev.cpu_usage_usec) as f64;
    // 1 core-second = 1_000_000 usec. Millicores = (usec / 1_000_000 / elapsed) * 1000
    (delta_usec / 1_000_000.0 / elapsed) * 1000.0
}
```

**Flush loop** — matches the pattern in `metrics::flush_red_metrics()` (`tokio::select!` over ticker + shutdown). Note: no PID needed — cgroup stats are container-scoped (the proxy IS the container's PID 1):

```rust
pub async fn flush_process_metrics(
    service: String,
    metric_tx: mpsc::Sender<MetricRecord>,
    interval: Duration,
    mut shutdown: watch::Receiver<()>,
) {
    let mut prev: Option<CgroupSnapshot> = None;
    let mut ticker = tokio::time::interval(interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Some(curr) = read_cgroup_stats() {
                    let labels = serde_json::json!({"service": &service});
                    // Memory metric (container-scoped)
                    let _ = metric_tx.try_send(MetricRecord {
                        name: "process.memory.rss".into(), labels: labels.clone(),
                        metric_type: "gauge".into(), unit: Some("bytes".into()),
                        timestamp: Utc::now(), value: curr.mem_bytes as f64,
                    });
                    // CPU metric (needs previous snapshot for delta)
                    if let Some(ref p) = prev {
                        let mc = cpu_millicores(p, &curr);
                        let _ = metric_tx.try_send(MetricRecord {
                            name: "process.cpu.utilization".into(), labels: labels.clone(),
                            metric_type: "gauge".into(), unit: Some("millicores".into()),
                            timestamp: Utc::now(), value: mc,
                        });
                    }
                    prev = Some(curr);
                }
            }
            _ = shutdown.changed() => break,
        }
    }
}
```

In `crates/proxy/src/main.rs`, after the health server spawn — no PID argument needed since cgroups are container-scoped:
```rust
tokio::spawn(process_metrics::flush_process_metrics(
    config.service_name.clone(), metric_tx.clone(),
    Duration::from_secs(config.metrics_interval), shutdown_rx.clone(),
));
```

**Fallback behavior:**
- Containerized Linux (K8s): reads cgroup v2 or v1 → emits metrics
- Non-containerized Linux: cgroup paths may exist (systemd slices) → emits metrics
- macOS / no cgroup: `read_cgroup_stats()` returns `None` → no metrics emitted, UI shows "N/A"

### Code Changes — PR 1

> **Note:** The proxy crate lives at `crates/proxy/`, not `src/proxy/`. The old `src/proxy/` was a stale copy and has been deleted.

| File | Change |
|---|---|
| `crates/proxy/src/proxy/process_metrics.rs` | **New** — `read_cgroup_stats()`, `read_cgroup_v2()`, `read_cgroup_v1()`, `cpu_millicores()`, `flush_process_metrics()` |
| `crates/proxy/src/main.rs` | Spawn process metrics task (~3 lines, no PID needed) |
| `crates/proxy/src/proxy/mod.rs` | Add `pub mod process_metrics;` |
| `src/proxy/` | **Deleted** — stale copy, not compiled by any crate |

### Test Strategy — PR 1

Tests in `#[cfg(test)] mod tests` inside `crates/proxy/src/proxy/process_metrics.rs`, matching `metrics.rs` pattern. Cgroup filesystem tests gated with `#[cfg(target_os = "linux")]`.

| Test | Validates | Tier |
|---|---|---|
| `cpu_millicores_basic` | 1_000_000 usec delta / 1s elapsed = 1000 millicores (1 full core) | Unit |
| `cpu_millicores_zero_elapsed` | Returns 0.0 when elapsed is 0 (no div-by-zero) | Unit |
| `cpu_millicores_no_delta` | Returns 0.0 when usage_usec hasn't changed | Unit |
| `cpu_millicores_saturating` | Handles prev > curr gracefully (saturating_sub) | Unit |
| `cpu_millicores_fractional` | 500_000 usec delta / 1s = 500 millicores (half core) | Unit |
| `read_cgroup_stats_container` | Inside K8s pod: returns `Some(snap)` with `mem_bytes > 0` | Unit (Linux only) |
| `read_cgroup_stats_fallback_none` | When no cgroup paths exist, returns `None` (no panic) | Unit |
| `read_cgroup_v2_parses_cpu_stat` | Parse `"usage_usec 12345\nuser_usec 10000\nsystem_usec 2345\n"` → 12345 | Unit |

**Total: 8 unit tests**

---

## PR 2: Backend — New Observe Aggregation Endpoints

5 new read-only query endpoints in `src/observe/query.rs`. Uses dynamic `sqlx::query()` (not compile-time macros).

- [x] Types & errors defined
- [x] Tests written (red phase)
- [x] Implementation complete (green phase)
- [x] Integration/E2E tests passing
- [x] Quality gate passed

> **Deviation:** Deploy markers query joins `deploy_releases` → `deploy_targets` for `environment` column (not on releases table directly). PERCENTILE_CONT casts fixed: `(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY duration_ms))::float8` — the `::float8` must wrap the whole expression, not just `duration_ms`. Same for error_rate numeric→float8 cast. Extracted `fetch_sparkline_history()` helper from `get_components` to stay under 100-line clippy limit.

### Route Ordering Note

`/api/observe/traces/aggregated` must be registered **before** `/api/observe/traces/{trace_id}` in the router. Axum resolves static segments over path params at the same depth, but only when they appear first.

### Endpoints

#### 1. Service Topology — `GET /api/observe/topology`

**Params:** `project_id?`, `range?` (default "1h"), `from?`, `to?`

**Response:**
```rust
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TopologyEdge {
    pub from_service: String,
    pub to_service: String,
    pub call_count: i64,
    pub error_count: i64,
    pub p50_ms: f64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TopologyResponse {
    pub edges: Vec<TopologyEdge>,
    pub services: Vec<String>,
}
```

**SQL:**
```sql
WITH edges AS (
    SELECT c.service AS from_service, s.service AS to_service,
        s.status, s.duration_ms
    FROM spans c
    JOIN spans s ON c.trace_id = s.trace_id
        AND s.kind = 'server' AND c.service != s.service
    WHERE c.kind = 'client'
      AND c.started_at >= $1
      AND ($2::timestamptz IS NULL OR c.started_at <= $2)
      AND c.project_id IS NOT DISTINCT FROM $3::uuid
)
SELECT from_service, to_service,
    COUNT(*) AS call_count,
    COUNT(*) FILTER (WHERE status = 'error') AS error_count,
    COALESCE(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY duration_ms)::float8, 0) AS p50_ms
FROM edges GROUP BY from_service, to_service
ORDER BY call_count DESC LIMIT 100
```

Services list: `SELECT DISTINCT service FROM spans WHERE started_at >= $1 ...`

**Permission:** `require_observe_read(state, auth, params.project_id)`

#### 2. Error Breakdown — `GET /api/observe/errors`

**Params:** `project_id?`, `range?`, `from?`, `to?`, `limit?` (default 50, enforce `.min(100)`)

**Response:**
```rust
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ErrorGroup {
    pub error_type: String,
    pub endpoint: String,
    pub downstream: String,
    pub count: i64,
    pub last_seen: DateTime<Utc>,
}
```

**SQL — safe cast** (avoids `::int` on potentially non-numeric JSONB values):
```sql
SELECT
    CASE
        WHEN attributes->>'http.status_code' ~ '^\d+$'
             AND (attributes->>'http.status_code')::int >= 500
            THEN (attributes->>'http.status_code') || ' Server Error'
        WHEN attributes->>'http.status_code' ~ '^\d+$'
             AND (attributes->>'http.status_code')::int >= 400
            THEN (attributes->>'http.status_code') || ' Client Error'
        ELSE 'Error'
    END AS error_type,
    name AS endpoint,
    COUNT(*) AS count,
    MAX(started_at) AS last_seen
FROM spans
WHERE status = 'error' AND kind = 'server'
  AND started_at >= $1
  AND ($2::timestamptz IS NULL OR started_at <= $2)
  AND project_id IS NOT DISTINCT FROM $3::uuid
GROUP BY error_type, endpoint
ORDER BY count DESC LIMIT $4
```

**Note:** The `~ '^\d+$'` check ensures the JSONB value is numeric before casting — prevents runtime errors on malformed attribute values.

#### 3. Trace Aggregation — `GET /api/observe/traces/aggregated`

**Params:** `project_id?`, `range?`, `from?`, `to?`, `limit?` (default 50, enforce `.min(100)`)

**Response:**
```rust
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TraceAggRow {
    pub name: String,
    pub count: i64,
    pub avg_duration_ms: f64,
    pub error_rate: f64,
    pub p99_duration_ms: f64,
}
```

**SQL:** (traces table has `project_id` directly — no subquery needed)
```sql
SELECT root_span AS name, COUNT(*) AS count,
    COALESCE(AVG(duration_ms)::float8, 0) AS avg_duration_ms,
    COALESCE(100.0 * COUNT(*) FILTER (WHERE status = 'error')
        / NULLIF(COUNT(*), 0), 0) AS error_rate,
    COALESCE(PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY duration_ms)::float8, 0)
        AS p99_duration_ms
FROM traces
WHERE started_at >= $1
  AND ($2::timestamptz IS NULL OR started_at <= $2)
  AND project_id IS NOT DISTINCT FROM $3::uuid
GROUP BY root_span ORDER BY count DESC LIMIT $4
```

#### 4. Request Load Timeline — `GET /api/observe/load`

**Params:** `project_id?`, `range?` (default "1h"), `from?`, `to?`, `buckets?` (default 120, **enforce `.min(500)` to prevent DoS**)

**Response:**
```rust
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LoadPoint {
    pub ts: DateTime<Utc>,
    pub rps: f64,
    pub errors: f64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DeployMarker {
    pub ts: DateTime<Utc>,
    pub image: String,
    pub env: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LoadResponse {
    pub points: Vec<LoadPoint>,
    pub deploys: Vec<DeployMarker>,
}
```

**Metric samples SQL** (RED metrics are snapshot-and-reset counters — each sample IS the delta):
```sql
WITH bucketed AS (
    SELECT width_bucket(
            EXTRACT(EPOCH FROM ms.timestamp),
            EXTRACT(EPOCH FROM $1::timestamptz),
            EXTRACT(EPOCH FROM $2::timestamptz), $3::int
        ) AS bucket, ms.value, ser.name
    FROM metric_samples ms
    JOIN metric_series ser ON ser.id = ms.series_id
    WHERE ser.name IN ('http.server.request.count', 'http.server.error.count')
      AND ser.project_id IS NOT DISTINCT FROM $4::uuid
      AND ms.timestamp >= $1 AND ms.timestamp <= $2
)
SELECT bucket,
    COALESCE(SUM(value) FILTER (WHERE name = 'http.server.request.count'), 0) AS rps,
    COALESCE(SUM(value) FILTER (WHERE name = 'http.server.error.count'), 0) AS errors
FROM bucketed WHERE bucket >= 1 AND bucket <= $3
GROUP BY bucket ORDER BY bucket
```

Deploy markers:
```sql
SELECT image_ref AS image, environment AS env, started_at AS ts
FROM deploy_releases
WHERE project_id IS NOT DISTINCT FROM $1::uuid
  AND started_at >= $2 AND ($3::timestamptz IS NULL OR started_at <= $3)
ORDER BY started_at DESC LIMIT 50
```

#### 5. Component Health — `GET /api/observe/components`

##### Architecture: Background Push, not Pull on Request

The component health endpoint must **not** call the K8s API at request time. If the K8s API is down or slow, observability should be the last thing to break. Instead, a background watcher streams K8s events via `kube::runtime::reflector` into an in-memory cache and flushes state as gauge metrics into `metric_samples` every 30s. The endpoint becomes a pure, fast SQL query.

##### K8s Watcher — `src/observe/k8s_watcher.rs` (new file)

Event-driven architecture using `kube::runtime::reflector` + `watcher`. Instead of polling `api.list()` every 30s (which fetches the full resource list each time), we stream K8s events in real-time. The reflector maintains a thread-safe in-memory `Store` that's always up-to-date with zero-latency reads. A 30s flush loop reads the local cache and writes gauge metrics to Postgres — no K8s API call at flush time.

**Why reflector over polling:**
- K8s API load is O(delta) not O(total) — only streaming diffs via WATCH, not mass-listing
- State updates arrive in real-time (pod crash → instant cache update)
- The flush loop reads local memory, completely decoupled from K8s network
- Existing pattern in the codebase: `crates/proxy/src/proxy/gateway/watcher.rs` already uses `kube::runtime::watcher`

**Metrics produced:**

| K8s Property | Metric Name | Labels | Value |
|---|---|---|---|
| Pod Ready condition | `k8s.pod.ready` | `service`, `pod` | 1 or 0 |
| Container restarts | `k8s.pod.restarts` | `service`, `pod` | cumulative int |
| OOMKilled reason | `k8s.pod.oom_kills` | `service`, `pod` | cumulative int |
| Deployment replicas | `k8s.deployment.replicas` | `service` | int |
| Ready replicas | `k8s.deployment.ready_replicas` | `service` | int |
| CPU request | `k8s.container.cpu.request` | `service` | millicores |
| CPU limit | `k8s.container.cpu.limit` | `service` | millicores |
| Memory request | `k8s.container.memory.request` | `service` | bytes |
| Memory limit | `k8s.container.memory.limit` | `service` | bytes |

**Implementation:**

```rust
// src/observe/k8s_watcher.rs

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, api::ListParams};
use kube::runtime::{reflector, watcher, WatchStreamExt};
use futures::StreamExt;

/// Background task: stream K8s events into in-memory reflector stores,
/// flush state as gauge metrics to metric_samples every 30s.
#[tracing::instrument(skip_all, fields(namespace = %namespace))]
pub async fn run(
    state: AppState,
    namespace: String,
    mut shutdown: tokio::sync::watch::Receiver<()>,
) {
    let pods_api: Api<Pod> = Api::namespaced(state.kube.clone(), &namespace);
    let deps_api: Api<Deployment> = Api::namespaced(state.kube.clone(), &namespace);

    // Set up reflector stores (thread-safe in-memory caches)
    let (pod_store, pod_writer) = reflector::store();
    let (dep_store, dep_writer) = reflector::store();

    // Stream K8s events into the reflector caches.
    // Spawn stream consumers in the background so they run freely —
    // avoids tokio::select! starvation between two noisy streams
    // and the overhead of dropping/recreating futures each loop iteration.
    let pod_stream = reflector::reflector(pod_writer,
        watcher(pods_api, watcher::Config::default()))
        .default_backoff()
        .applied_objects();
    let dep_stream = reflector::reflector(dep_writer,
        watcher(deps_api, watcher::Config::default()))
        .default_backoff()
        .applied_objects();

    tokio::spawn(async move {
        tokio::pin!(pod_stream);
        while pod_stream.next().await.is_some() {}
    });
    tokio::spawn(async move {
        tokio::pin!(dep_stream);
        while dep_stream.next().await.is_some() {}
    });

    // Flush loop: read local in-memory stores, write to DB.
    // The select! only handles the timer and shutdown — no stream contention.
    let mut ticker = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = flush_stores(&state, &pod_store, &dep_store).await {
                    tracing::warn!(error = %e, "k8s metric flush failed");
                }
            }
            _ = shutdown.changed() => break,
        }
    }
}

/// Read the in-memory reflector stores and write metrics to Postgres.
/// No K8s API calls — purely local memory reads + DB writes.
async fn flush_stores(
    state: &AppState,
    pod_store: &reflector::Store<Pod>,
    dep_store: &reflector::Store<Deployment>,
) -> anyhow::Result<()> {
    let mut metrics = Vec::new();

    // Deployments → replicas, ready_replicas
    for dep in dep_store.state() {
        let name = dep.metadata.name.as_deref().unwrap_or("");
        let labels = serde_json::json!({"service": name});
        let status = dep.status.as_ref();
        push_gauge(&mut metrics, "k8s.deployment.replicas", &labels,
            dep.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0) as f64);
        push_gauge(&mut metrics, "k8s.deployment.ready_replicas", &labels,
            status.and_then(|s| s.ready_replicas).unwrap_or(0) as f64);
    }

    // Pods → restarts, OOM kills, ready status, resource requests/limits
    for pod in pod_store.state() {
        let service = pod_owner_name(&pod);
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let labels = serde_json::json!({"service": service, "pod": pod_name});

        let mut restarts: i32 = 0;
        let mut ooms: i32 = 0;
        if let Some(status) = &pod.status {
            for cs in status.container_statuses.iter().flatten() {
                restarts += cs.restart_count;
                if let Some(last) = &cs.last_state {
                    if let Some(term) = &last.terminated {
                        if term.reason.as_deref() == Some("OOMKilled") {
                            ooms += 1;
                        }
                    }
                }
            }
            let is_ready = status.conditions.iter().flatten()
                .any(|c| c.type_ == "Ready" && c.status == "True");
            push_gauge(&mut metrics, "k8s.pod.ready", &labels,
                if is_ready { 1.0 } else { 0.0 });
        }
        push_gauge(&mut metrics, "k8s.pod.restarts", &labels, restarts as f64);
        push_gauge(&mut metrics, "k8s.pod.oom_kills", &labels, ooms as f64);

        // Resource requests/limits from pod spec
        if let Some(spec) = &pod.spec {
            for container in &spec.containers {
                if let Some(res) = &container.resources {
                    let svc_labels = serde_json::json!({"service": service});
                    push_gauge(&mut metrics, "k8s.container.cpu.request", &svc_labels,
                        parse_cpu(res.requests.as_ref().and_then(|r| r.get("cpu"))));
                    push_gauge(&mut metrics, "k8s.container.cpu.limit", &svc_labels,
                        parse_cpu(res.limits.as_ref().and_then(|r| r.get("cpu"))));
                    push_gauge(&mut metrics, "k8s.container.memory.request", &svc_labels,
                        parse_mem(res.requests.as_ref().and_then(|r| r.get("memory"))));
                    push_gauge(&mut metrics, "k8s.container.memory.limit", &svc_labels,
                        parse_mem(res.limits.as_ref().and_then(|r| r.get("memory"))));
                }
            }
        }
    }

    store::write_metrics(&state.pool, &metrics).await?;
    Ok(())
}

/// Parse K8s CPU quantity string to millicores.
/// "500m" → 500.0, "2" → 2000.0, "0.5" → 500.0
fn parse_cpu(quantity: Option<&k8s_openapi::apimachinery::pkg::api::resource::Quantity>) -> f64 {
    let s = match quantity { Some(q) => &q.0, None => return 0.0 };
    if let Some(millis) = s.strip_suffix('m') {
        millis.parse::<f64>().unwrap_or(0.0)
    } else {
        // Whole cores or fractional: "2" → 2000, "0.5" → 500
        s.parse::<f64>().unwrap_or(0.0) * 1000.0
    }
}

/// Parse K8s memory quantity string to bytes.
/// "512Mi" → 536870912, "1Gi" → 1073741824, "1000000" → 1000000
fn parse_mem(quantity: Option<&k8s_openapi::apimachinery::pkg::api::resource::Quantity>) -> f64 {
    let s = match quantity { Some(q) => &q.0, None => return 0.0 };
    if let Some(v) = s.strip_suffix("Ki") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0
    } else if let Some(v) = s.strip_suffix("Mi") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix("Gi") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix("Ti") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0 * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix('K') {  // SI: 10^3
        v.parse::<f64>().unwrap_or(0.0) * 1000.0
    } else if let Some(v) = s.strip_suffix('M') {  // SI: 10^6
        v.parse::<f64>().unwrap_or(0.0) * 1_000_000.0
    } else if let Some(v) = s.strip_suffix('G') {  // SI: 10^9
        v.parse::<f64>().unwrap_or(0.0) * 1_000_000_000.0
    } else {
        // Raw bytes
        s.parse::<f64>().unwrap_or(0.0)
    }
}
```

**Spawn at startup** in `src/main.rs` `spawn_background_tasks()`:
```rust
tokio::spawn(observe::k8s_watcher::run(
    state.clone(),
    state.config.platform_namespace.clone(),
    shutdown_tx.subscribe(),
));
```

For project namespaces: the watcher can be extended to iterate all active namespaces or spawn per-namespace watchers. For v1, we watch the platform namespace only — project component health comes later.

##### API Endpoint — Pure SQL

With K8s state stored as metrics, the endpoint is a single fast query — zero K8s API calls.

**Params:** `project_id?`

**Response:**
```rust
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ComponentHealth {
    pub name: String,
    pub ready: bool,
    pub live: bool,
    pub replicas: i32,
    pub ready_replicas: i32,
    pub restarts: i32,
    pub oom_kills: i32,
    pub cpu_used_millicores: f64,
    pub cpu_request: i64,
    pub cpu_limit: i64,
    pub mem_used_bytes: f64,
    pub mem_request: i64,
    pub mem_limit: i64,
    pub active_requests: f64,
    pub avg_rps: f64,
    pub cpu_history: Vec<f64>,
    pub mem_history: Vec<f64>,
    pub rps_history: Vec<f64>,
}
```

**SQL — single query over `metric_series`/`metric_samples`:**

```sql
-- Latest value per (service, metric_name) for component summary
WITH latest AS (
    SELECT
        ser.labels->>'service' AS service,
        ser.name AS metric_name,
        (SELECT ms.value FROM metric_samples ms
         WHERE ms.series_id = ser.id
         ORDER BY ms.timestamp DESC LIMIT 1) AS val
    FROM metric_series ser
    WHERE ser.project_id IS NOT DISTINCT FROM $1::uuid
      AND ser.name IN (
          'k8s.deployment.replicas', 'k8s.deployment.ready_replicas',
          'k8s.pod.restarts', 'k8s.pod.oom_kills', 'k8s.pod.ready',
          'k8s.container.cpu.request', 'k8s.container.cpu.limit',
          'k8s.container.memory.request', 'k8s.container.memory.limit',
          'process.cpu.utilization', 'process.memory.rss',
          'http.server.request.count'
      )
)
SELECT
    service AS name,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.pod.ready'), 0) > 0 AS ready,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.deployment.replicas'), 0)::int AS replicas,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.deployment.ready_replicas'), 0)::int AS ready_replicas,
    COALESCE(SUM(val) FILTER (WHERE metric_name = 'k8s.pod.restarts'), 0)::int AS restarts,
    COALESCE(SUM(val) FILTER (WHERE metric_name = 'k8s.pod.oom_kills'), 0)::int AS oom_kills,
    COALESCE(AVG(val) FILTER (WHERE metric_name = 'process.cpu.utilization'), 0) AS cpu_used_millicores,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.container.cpu.request'), 0)::bigint AS cpu_request,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.container.cpu.limit'), 0)::bigint AS cpu_limit,
    COALESCE(AVG(val) FILTER (WHERE metric_name = 'process.memory.rss'), 0) AS mem_used_bytes,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.container.memory.request'), 0)::bigint AS mem_request,
    COALESCE(MAX(val) FILTER (WHERE metric_name = 'k8s.container.memory.limit'), 0)::bigint AS mem_limit,
    COALESCE(AVG(val) FILTER (WHERE metric_name = 'http.server.request.count'), 0) AS avg_rps
FROM latest
GROUP BY service
```

Sparkline history — separate query, last 20 samples per (service, metric):
```sql
SELECT ser.labels->>'service' AS service, ser.name, ms.value, ms.timestamp
FROM metric_samples ms
JOIN metric_series ser ON ser.id = ms.series_id
WHERE ser.name IN ('process.cpu.utilization', 'process.memory.rss', 'http.server.request.count')
  AND ser.project_id IS NOT DISTINCT FROM $1::uuid
  AND ms.timestamp >= NOW() - INTERVAL '10 minutes'
ORDER BY ms.timestamp DESC
```

Group by `(service, metric_name)` in Rust, take last 20 per group.

**Benefits of background push over pull-on-request:**
- If K8s API is down, the endpoint still returns last-known state from DB
- Response time is ~5ms (SQL only) vs ~500ms+ (K8s API calls)
- Historical data available for sparklines automatically
- No request-time timeout risk
- Observability is decoupled from control plane health

### Route Registration

**CRITICAL — Axum route ordering:** `/api/observe/traces/aggregated` **MUST** be registered before `/api/observe/traces/{trace_id}`. Axum's router is strict: a parameterized path (`{trace_id}`) at the same depth will swallow the literal `"aggregated"` and try to parse it as a UUID, returning a 400/404. Static segments must come first.

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        // ⚠️ MUST come before /api/observe/traces/{trace_id} — axum parses
        // "aggregated" as {trace_id} if the parameterized route is first
        .route("/api/observe/traces/aggregated", get(get_trace_aggregation))
        // ... existing routes (including /api/observe/traces/{trace_id}) ...
        .route("/api/observe/topology", get(get_topology))
        .route("/api/observe/errors", get(get_error_breakdown))
        .route("/api/observe/load", get(get_load_timeline))
        .route("/api/observe/components", get(get_components))
}
```

### Log Templates — Client-Side

Template grouping stays client-side in ObserveTab (replace numbers/UUIDs/IPs with `{...}` placeholders). Avoids server-side NLP.

### Code Changes — PR 2

| File | Change |
|---|---|
| `src/observe/k8s_watcher.rs` | **New** — event-driven K8s reflector, streams pod/deployment state into in-memory cache, flushes gauge metrics every 30s |
| `src/observe/mod.rs` | Add `pub mod k8s_watcher;` |
| `src/main.rs` | Spawn `k8s_watcher::run()` in `spawn_background_tasks()` |
| `src/observe/query.rs` | Add 5 handlers + param/response types + route registration. ~400 lines. Component health is pure SQL (no K8s calls). |
| `ui/src/lib/types.ts` | Re-export generated types |

### Test Strategy — PR 2

Tests in `tests/observe_integration.rs`, using `test_state(pool)` + `test_router(state)` + `platform::observe::store::write_spans/write_metrics` to insert test data.

**Topology tests:**

| Test | Validates | Tier |
|---|---|---|
| `topology_happy_path` | Insert client+server span pairs across services → edges returned with counts | Integration |
| `topology_empty` | No spans → `{"edges":[], "services":[]}` | Integration |
| `topology_requires_admin_global` | No project_id without admin → 403 | Integration |
| `topology_error_edges` | Server span with `status=error` → edge has `error_count > 0` | Integration |

**Error breakdown tests:**

| Test | Validates | Tier |
|---|---|---|
| `errors_happy_path` | Insert error spans → grouped by status code + endpoint | Integration |
| `errors_empty` | No error spans → empty response | Integration |
| `errors_time_range` | Old errors excluded by `from`/`to` filter | Integration |
| `errors_permission` | Unprivileged user → 403 | Integration |

**Trace aggregation tests:**

| Test | Validates | Tier |
|---|---|---|
| `trace_agg_happy_path` | Insert 10 traces with varying duration → correct avg/p99/error_rate | Integration |
| `trace_agg_empty` | No traces → empty response | Integration |
| `trace_agg_project_scoped` | Only matching project's traces in result | Integration |
| `trace_agg_permission` | Unprivileged → 403 | Integration |

**Load timeline tests:**

| Test | Validates | Tier |
|---|---|---|
| `load_happy_path` | Insert request.count + error.count samples → bucketed points | Integration |
| `load_empty` | No metrics → empty points | Integration |
| `load_deploy_markers` | Insert deploy_releases → markers in response | Integration |
| `load_permission` | Unprivileged → 403 | Integration |

**Component health tests:**

| Test | Validates | Tier |
|---|---|---|
| `components_happy_path` | Insert k8s.* + process.* metrics for 2 services → grouped component response | Integration |
| `components_empty` | No metrics → empty list | Integration |
| `components_admin_required` | No project_id without admin → 403 | Integration |
| `components_sparkline_history` | Insert 20 process.cpu samples → cpu_history has 20 values | Integration |
| `components_aggregates_pods` | Insert k8s.pod.restarts for 3 pods of same service → summed in response | Integration |

**K8s watcher tests:**

| Test | Validates | Tier |
|---|---|---|
| `k8s_watcher_writes_deployment_metrics` | Watcher reflector populates store, flush writes k8s.deployment.replicas to DB | Integration |
| `k8s_watcher_writes_pod_metrics` | Watcher reflector populates store, flush writes k8s.pod.ready/restarts to DB | Integration |

**K8s quantity parser tests (unit, in `k8s_watcher.rs`):**

| Test | Validates | Tier |
|---|---|---|
| `parse_cpu_millicores` | `"500m"` → 500.0 | Unit |
| `parse_cpu_whole_cores` | `"2"` → 2000.0 | Unit |
| `parse_cpu_fractional` | `"0.5"` → 500.0 | Unit |
| `parse_cpu_none` | `None` → 0.0 | Unit |
| `parse_mem_mebibytes` | `"512Mi"` → 536870912.0 | Unit |
| `parse_mem_gibibytes` | `"1Gi"` → 1073741824.0 | Unit |
| `parse_mem_kibibytes` | `"256Ki"` → 262144.0 | Unit |
| `parse_mem_si_mega` | `"500M"` → 500000000.0 (SI, not MiB) | Unit |
| `parse_mem_raw_bytes` | `"1048576"` → 1048576.0 | Unit |
| `parse_mem_none` | `None` → 0.0 | Unit |

**Cross-cutting:**

| Test | Validates | Tier |
|---|---|---|
| `all_new_endpoints_permission` | Verify all 5 endpoints return 403 for unprivileged user | Integration |

**Total: 24 integration + 10 unit tests**

**Data insertion helpers:**
```rust
async fn insert_span_full(pool: &PgPool, trace_id: &str, span_id: &str,
    service: &str, kind: &str, status: &str, duration_ms: i32, project_id: Option<Uuid>) {
    let span = SpanRecord { trace_id: trace_id.into(), span_id: span_id.into(),
        parent_span_id: None, name: "test-op".into(), service: service.into(),
        kind: kind.into(), status: status.into(), attributes: None, events: None,
        duration_ms: Some(duration_ms), started_at: Utc::now(),
        finished_at: Some(Utc::now() + chrono::Duration::milliseconds(duration_ms.into())),
        project_id, session_id: None, user_id: None,
    };
    write_spans(pool, &[span]).await.unwrap();
}
```

Use UUID-tagged service names per test (`format!("svc-{}", Uuid::new_v4().simple())`) for parallel isolation.

---

## PR 3: Frontend — Rename Mock, Build New Live ObserveTab

Keep the existing mock component as a visual reference on project pages. Build a new `ObserveTab` wired to real APIs alongside it. This lets us compare side-by-side while iterating.

- [x] Mock renamed
- [x] New live component created
- [x] Operations page wired
- [x] Quality gate passed

### Step 1: Rename existing mock

Rename `ObserveTab` → `ObserveTabMock` so the mock stays as-is on project pages:

```
ui/src/components/ObserveTab.tsx → ui/src/components/ObserveTabMock.tsx
```

Update the one import in `ProjectTabs.tsx` (or `ProjectCard.tsx`) to use `ObserveTabMock`:
```tsx
// Before:
import { ObserveTab } from './ObserveTab';
// After:
import { ObserveTabMock } from './ObserveTabMock';
```

The project observe tab keeps showing mock data exactly as before — it's the visual target we're building toward.

### Step 2: Build new `ObserveTab` (live data)

New file: `ui/src/components/ObserveTab.tsx` — same layout/structure as the mock, but wired to real APIs.

**Props:**
```tsx
interface ObserveTabProps {
    projectId?: string;
    platformMode?: boolean;  // true = platform namespace, admin required
}
```

**API mapping** (each mock function → real API call):

| Section | API Call | Notes |
|---|---|---|
| Component health | `GET /api/observe/components?project_id=...` | New endpoint (PR 2), auto-refresh 30s |
| Communication graph | `GET /api/observe/topology?project_id=...&range=1h` | New endpoint (PR 2) |
| Request load + deploys | `GET /api/observe/load?project_id=...&range=1h` | New endpoint (PR 2) |
| Error breakdown | `GET /api/observe/errors?project_id=...&range=1h` | New endpoint (PR 2) |
| Alerts | `GET /api/observe/alerts?project_id=...` | Existing endpoint |
| Traces (recent) | `GET /api/observe/traces?project_id=...&limit=20` | Existing endpoint |
| Traces (aggregated) | `GET /api/observe/traces/aggregated?project_id=...` | New endpoint (PR 2) |
| Trace waterfall | `GET /api/observe/traces/{id}` | Existing endpoint |
| Log templates | `GET /api/observe/logs?limit=500` + client-side grouping | Template extraction in JS |
| System logs | `GET /api/observe/logs?source=system&limit=20` | Existing with filter |
| CPU chart | `GET /api/observe/metrics/query?name=process.cpu.utilization` | Existing endpoint, new metric (PR 1) |
| Memory chart | `GET /api/observe/metrics/query?name=process.memory.rss` | Existing endpoint, new metric (PR 1) |
| Response time chart | `GET /api/observe/metrics/query?name=http.server.request.duration_sum` | Existing endpoint |
| SLO bar | Mock data (hardcoded) | SLO engine deferred |

**Data fetching pattern:**

**Range→timestamp contract:** The frontend passes `range=1h` (or `5m`, `6h`, `24h`, `7d`). The backend's `resolve_range()` helper (already in `query.rs:322-340`) converts this to an absolute `from` timestamp (`now() - duration`). The backend SQL binds `$1 = from`, `$2 = to` (or NULL for open-ended). The frontend must **not** compute timestamps itself — pass the range string and let the backend resolve it server-side to avoid clock skew between browser and server.

```tsx
useEffect(() => {
    const range = presetRange; // "1h", "6h", "24h" etc — backend resolves to timestamps
    Promise.all([
        api.get<ComponentHealth[]>(`/api/observe/components?${scope}`),
        api.get<TopologyResponse>(`/api/observe/topology?${scope}&range=${range}`),
        api.get<LoadResponse>(`/api/observe/load?${scope}&range=${range}`),
        api.get<ErrorGroup[]>(`/api/observe/errors?${scope}&range=${range}`),
        api.get<ListResponse<AlertRule>>(`/api/observe/alerts?${scope}`),
    ]).then(([comp, topo, ld, errs, alerts]) => { ... });
}, [projectId, presetRange]);
```

**Client-side log template extraction:**
```tsx
function extractTemplates(logs: LogEntry[]): LogTemplate[] {
    const templateOf = (msg: string) =>
        msg.replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, '{id}')
           .replace(/\b\d+\.\d+\.\d+\.\d+\b/g, '{ip}')
           .replace(/\b\d+(\.\d+)?\b/g, '{n}')
           .replace(/\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g, '{email}');
    const groups = new Map<string, { count: number; level: string; sample: string }>();
    for (const log of logs) {
        const tmpl = templateOf(log.message);
        const existing = groups.get(tmpl);
        if (existing) existing.count++;
        else groups.set(tmpl, { count: 1, level: log.level, sample: log.message });
    }
    return Array.from(groups, ([template, v]) => ({ template, ...v }))
        .sort((a, b) => b.count - a.count);
}
```

**Platform mode behavior:**
- Hide staging/production toggle (single platform namespace)
- Omit `project_id` from all API calls (global scope, admin required)
- Show loading/error/empty states for each section

### Step 3: Wire up Operations page + navigation

New file: `ui/src/pages/observe/Operations.tsx` — platform-level Operations page with project selector:

```tsx
import { useState, useEffect } from 'preact/hooks';
import { api, type ListResponse } from '../../lib/api';
import type { Project } from '../../lib/types';
import { ObserveTab } from '../../components/ObserveTab';

export function Operations() {
    const [projectId, setProjectId] = useState<string | undefined>(undefined);
    const [projects, setProjects] = useState<Project[]>([]);
    useEffect(() => {
        api.get<ListResponse<Project>>('/api/projects?limit=100')
            .then(r => setProjects(r.items)).catch(() => {});
    }, []);
    return (
        <div>
            <div class="page-header" style="display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem">
                <h2 style="margin:0">Operations</h2>
                <select class="input" style="width:auto;min-width:200px"
                    value={projectId || ''} onChange={e => {
                        const v = (e.target as HTMLSelectElement).value;
                        setProjectId(v || undefined);
                    }}>
                    <option value="">Platform Infrastructure</option>
                    {projects.map(p => (
                        <option key={p.id} value={p.id}>{p.display_name || p.name}</option>
                    ))}
                </select>
            </div>
            <ObserveTab projectId={projectId} platformMode={!projectId} />
        </div>
    );
}
```

**Route changes** (`ui/src/index.tsx`): Add `/observe` route (keep old routes for now):
```tsx
import { Operations } from './pages/observe/Operations';
// Add alongside existing routes:
<Operations path="/observe" />
```

**Navigation** (`ui/src/components/Layout.tsx`): Add "Operations" to `OBSERVE_NAV`:
```tsx
const OBSERVE_NAV: NavItem[] = [
    { href: '/observe', label: 'Operations', icon: 'heart' },
    { href: '/observe/logs', label: 'Logs', icon: 'log' },
    { href: '/observe/traces', label: 'Traces', icon: 'trace' },
    { href: '/observe/metrics', label: 'Metrics', icon: 'chart' },
    { href: '/observe/alerts', label: 'Alerts', icon: 'bell' },
];
```

Keep the old 4 pages during this PR — they're still useful for drilling into raw data. They'll be removed in PR 5 once the live ObserveTab is complete.

### Code Changes — PR 3

| File | Change |
|---|---|
| `ui/src/components/ObserveTab.tsx` → `ObserveTabMock.tsx` | **Rename** — export as `ObserveTabMock`, no code changes |
| `ui/src/components/ProjectTabs.tsx` (or ProjectCard.tsx) | Update import: `ObserveTab` → `ObserveTabMock` |
| `ui/src/components/ObserveTab.tsx` | **New** — live version wired to real APIs |
| `ui/src/pages/observe/Operations.tsx` | **New** — page wrapper with project selector |
| `ui/src/index.tsx` | Add `<Operations path="/observe" />` route |
| `ui/src/components/Layout.tsx` | Add "Operations" link to `OBSERVE_NAV` |
| `ui/src/lib/types.ts` | Import generated types from PR 2 |

### Test Strategy — PR 3

| Test | Validates | Tier |
|---|---|---|
| `extractTemplates groups correctly` | Two identical messages → count=2, one unique → count=1 | Unit (JS) |
| `extractTemplates empty input` | Empty array → empty result | Unit (JS) |
| `extractTemplates UUID replacement` | Message with UUID → `{id}` placeholder | Unit (JS) |

**Total: 3 unit tests (JS)**

---

## PR 4: Iterate — Get Live ObserveTab to Parity with Mock

This is the iteration PR. Compare `/observe` (live) against `/projects/:id/observe` (mock reference) and close gaps. Each section of the live ObserveTab should visually match the mock.

- [x] Component health section matches mock
- [x] Communication graph section matches mock
- [x] Request load timeline matches mock
- [x] Error breakdown matches mock
- [x] Traces (ongoing/recent/aggregated) matches mock
- [x] Trace waterfall overlay matches mock
- [x] Log templates section matches mock
- [x] System logs section matches mock
- [x] Metrics charts (CPU/MEM/response time) match mock
- [x] Alerts section matches mock
- [x] All sections handle empty/loading/error states gracefully

### Working approach

Open two browser tabs side-by-side:
1. `/projects/<some-project>/observe` — the mock reference (ObserveTabMock)
2. `/observe` — the live Operations page (ObserveTab)

Iterate section by section, matching layout, styling, and interaction patterns. The mock has the exact CSS classes and structure to replicate.

### Code Changes — PR 4

| File | Change |
|---|---|
| `ui/src/components/ObserveTab.tsx` | Polish: fix layout gaps, loading states, empty states, styling to match mock |

---

## PR 5: Cleanup — Remove Mock, Wire Projects to Live, Delete Old Pages

Once the live ObserveTab is at parity with the mock, swap everything over and clean up.

- [ ] Projects use live ObserveTab
- [ ] Mock component deleted
- [ ] Old observe pages deleted
- [ ] Navigation cleaned up
- [ ] Quality gate passed

### Step 1: Wire project pages to live ObserveTab

Update `ProjectTabs.tsx` (or `ProjectCard.tsx`):
```tsx
// Before:
import { ObserveTabMock } from './ObserveTabMock';
// After:
import { ObserveTab } from './ObserveTab';
```

The live `ObserveTab` already accepts `projectId` — when rendered on a project page, it fetches project-scoped data.

### Step 2: Delete mock

```
rm ui/src/components/ObserveTabMock.tsx
```

### Step 3: Delete old observe pages + clean navigation

```
rm ui/src/pages/observe/Logs.tsx
rm ui/src/pages/observe/Traces.tsx
rm ui/src/pages/observe/Metrics.tsx
rm ui/src/pages/observe/Alerts.tsx
```

Update routes (`ui/src/index.tsx`):
```tsx
// Remove:
<Logs path="/observe/logs" />
<Traces path="/observe/traces" />
<TraceDetail path="/observe/traces/:traceId" />
<Metrics path="/observe/metrics" />
<Alerts path="/observe/alerts" />

// Keep:
<Operations path="/observe" />
```

Update navigation (`ui/src/components/Layout.tsx`):
```tsx
const OBSERVE_NAV: NavItem[] = [
    { href: '/observe', label: 'Operations', icon: 'heart' },
];
```

### Code Changes — PR 5

| File | Change |
|---|---|
| `ui/src/components/ProjectTabs.tsx` | Import `ObserveTab` instead of `ObserveTabMock` |
| `ui/src/components/ObserveTabMock.tsx` | **Delete** |
| `ui/src/pages/observe/Logs.tsx` | **Delete** |
| `ui/src/pages/observe/Traces.tsx` | **Delete** |
| `ui/src/pages/observe/Metrics.tsx` | **Delete** |
| `ui/src/pages/observe/Alerts.tsx` | **Delete** |
| `ui/src/index.tsx` | Remove 5 old observe routes, remove old imports |
| `ui/src/components/Layout.tsx` | Reduce `OBSERVE_NAV` to single "Operations" entry |

### Notes
- MCP server `platform-observe.js` calls `/api/observe/*` API endpoints — these are preserved, only UI pages deleted
- E2E tests hit API endpoints not UI routes — no breakage

---

## Summary

| PR | Scope | Key Change |
|---|---|---|
| PR 0 | Denormalize spans + indexes | Add project_id/session_id/user_id to spans, 7 indexes |
| PR 1 | Proxy: process metrics | New `process_metrics.rs` — CPU + MEM via cgroups (v2/v1 fallback) |
| PR 2 | Backend: K8s watcher + 5 endpoints | Event-driven K8s reflector → metric_samples; 5 query endpoints (all pure SQL) |
| PR 3 | Frontend: mock rename + live build + Operations page | `ObserveTab` → `ObserveTabMock`, new live `ObserveTab`, `/observe` route |
| PR 4 | Frontend: iterate to parity | Polish live ObserveTab section-by-section against mock reference |
| PR 5 | Cleanup: remove mock + old pages | Delete `ObserveTabMock`, delete 4 old pages, wire projects to live |

## Test Plan Summary

### New test counts by PR

| PR | Unit | Integration | E2E | Total |
|---|---|---|---|---|
| PR 0 | 0 | 2 | 0 | 2 |
| PR 1 | 8 | 0 | 0 | 8 |
| PR 2 | 10 | 24 | 0 | 34 |
| PR 3 | 3 | 0 | 0 | 3 |
| PR 4 | 0 | 0 | 0 | 0 |
| PR 5 | 0 | 0 | 0 | 0 |
| **Total** | **21** | **26** | **0** | **47** |

---

## Plan Review Findings

**Date:** 2026-04-09
**Status:** APPROVED WITH FIXES APPLIED

### Issues Found and Fixed In-Place

1. **CRITICAL — namespace IDOR** (Security): ComponentHealth originally accepted a `namespace` param allowing users to read pods from any K8s namespace. Fixed: namespace is derived from `project_id` or platform config, never user-supplied.

2. **Missing indexes + denormalization** (Performance): `spans` had no `project_id` column, forcing correlated subqueries against millions of rows. Added PR 0 to denormalize `project_id`/`session_id`/`user_id` onto spans (mirroring `log_entries` pattern) and add 7 composite indexes. The `SpanRecord` struct already carried this data — we were just not persisting it. No backfill needed (pre-alpha, no running installations).

3. **Unsafe JSONB cast** (Schema): `(attributes->>'http.status_code')::int` throws on non-numeric values. Fixed: added `~ '^\d+$'` regex check before casting.

4. **Unbounded `buckets` param** (Security/DoS): Load timeline accepted unlimited bucket count. Fixed: enforce `.min(500)`.

5. **K8s API coupling** (Architecture): Component health originally called K8s API at request time — if K8s is down, observability breaks first. Fixed: event-driven K8s reflector (`k8s_watcher.rs`) streams events into in-memory cache via `kube::runtime::reflector`, flushes gauge metrics every 30s. Endpoint is pure SQL (~5ms). If K8s API goes down, last-known state is still served from DB, and the reflector auto-reconnects when it comes back.

6. **Route ordering** (API): `/api/observe/traces/aggregated` would be captured by `/{trace_id}` param. Fixed: register static path before parameterized.

### Remaining Concerns

- **query.rs size**: Currently ~1030 lines, will grow to ~1430. Individual handlers must stay under 100 lines (clippy). Consider extracting a helper module if it gets unwieldy.
- **kube types isolated**: K8s API calls are now in `k8s_watcher.rs` only, not in `query.rs`. The query module stays pure-DB.
- **`PERCENTILE_CONT` on empty sets**: Returns NULL — all handlers use `COALESCE(..., 0)` which is correct.

### What's Well Done
- Zero new tables — pure query aggregation over existing data
- Proxy process metrics use the existing `MetricRecord` + OTLP channel pattern perfectly
- Client-side log templates avoid a complex server-side NLP endpoint
- Transparent proxy changes don't affect the telemetry format — plan correctly identifies data that's already flowing

### Deferred to Future Work

- **SLO definitions + engine** — new table + evaluation loop + burn-rate computation
- **Server-side log templates** — if client-side extraction is too slow for large volumes
- **"Ongoing" traces** — traces where `finished_at IS NULL`
- **Alert CRUD in ObserveTab** — read-only for v1, inline editing later
- **Project observe with live data** — PR 5 wires projects to the live ObserveTab. If project-scoped data needs different handling (e.g. env toggle for staging/prod namespaces), extend in a follow-up after PR 5
