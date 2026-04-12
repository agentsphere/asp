# Streaming Alert Evaluation

Replace the poll-based alert evaluator (query DB every 30s per rule) with a
stream-based approach using Valkey Streams. Each replica pushes matching metric
samples to a shared stream on ingest; a single consumer (leader via consumer
group) maintains in-memory sliding windows and evaluates alert conditions in
real-time.

---

## Current architecture (poll-based)

```
OTLP ingest ─► mpsc channel ─► flush_metrics() ─► Postgres
  (HTTP)         (10K cap)        (every 1s)      metric_series + metric_samples
                                                         ▲
                                                         │ SELECT AVG/SUM/...
                                                         │ WHERE timestamp > now() - window
                                                         │
                                  evaluate_alerts_loop ──┘
                                    (every 30s, sequential)
```

### What happens today (`src/observe/alert.rs`)

1. `evaluate_alerts_loop` sleeps 30s, then calls `evaluate_all()`.
2. `evaluate_all()` fetches all enabled rules (`LIMIT 500`) from `alert_rules`.
3. For each rule **sequentially**:
   - Parse the query DSL: `metric:<name> [labels:{json}] [agg:<func>] [window:<secs>]`
   - Call `evaluate_metric()` — a `SELECT {agg}(value) FROM metric_samples JOIN
     metric_series WHERE name = $1 AND labels @> $2 AND timestamp > now() - window`
   - Compare result against threshold via `check_condition()` (gt/lt/eq/absent)
   - Run state machine (`next_alert_state`) for `for_seconds` hold-off
   - Fire/resolve via `fire_alert()`/`resolve_alert()` + publish `AlertFired` event
4. Per-rule timeout: 10s. If a query is slow, that rule is skipped with a warning.

### HA behavior today

Every replica runs its own `evaluate_alerts_loop`. Each one queries Postgres
independently. Because the DB is the single source of truth, all replicas see
the same data and reach the same conclusions. The main problem is **duplicate
alerts** — two replicas both fire the same alert. The current code doesn't
deduplicate; `fire_alert()` is an INSERT (not upsert), so the worst case is
duplicate `alert_events` rows and duplicate event-bus publications.

Pipelines and deployer use `FOR UPDATE SKIP LOCKED` to claim discrete jobs.
Alerts are continuous evaluation — there's no row to claim.

### Problems

| Problem | Impact |
|---|---|
| **30s poll interval** is hardcoded | Best-case alert latency is 30s; worst-case 60s (just missed a cycle) |
| **Sequential rule evaluation** | 100 rules × avg 50ms query = 5s per cycle; 500 rules at 10s timeout = 83 min worst case |
| **DB pressure scales with rule count** | Every cycle runs N queries against `metric_samples` (partitioned, but still N index scans) |
| **Redundant work** | The flush task already saw every metric sample — we write to DB, then immediately read it back |
| **No rule-count scaling** | Adding rules linearly increases DB load per cycle |
| **Cold gap on restart** | In-memory `alert_states` HashMap is lost — `for_seconds` hold-off resets |
| **Duplicate alerts with 2+ replicas** | Each replica fires independently → duplicate events + notifications |

### What works well (keep)

- Query DSL (`parse_alert_query`) — simple, effective
- State machine (`next_alert_state`) — clean pure function, well-tested
- `for_seconds` hold-off logic
- `fire_alert()`/`resolve_alert()` + event bus publishing
- 500-rule cap with warning

---

## The multi-replica problem

With 2+ replicas behind a load balancer, each replica sees ~50% of metrics.
A pure in-memory accumulator per replica computes aggregates from partial data.
`AVG(cpu)` from half the samples is wrong; `COUNT` and `SUM` would be ~half
the real value.

Three approaches were considered:

| | **A: Leader + DB poll** | **B: Valkey sorted sets (poll)** | **C: Valkey Stream (push)** |
|---|---|---|---|
| Alert latency | 30–60s | 5–10s | **< 100ms** |
| Ingest overhead | 0 | N `ZADD` per matching sample | **1 `XADD`** per matching sample |
| Leader election | Advisory lock | Advisory lock | **Consumer group** (built-in) |
| Leader stateless? | Yes | Yes | No (ring buffers), but failover via consumer group |
| Backfill source | Postgres | Sorted sets per rule | Stream history (`XRANGE`) |
| Duplicate alert protection | Advisory lock | Advisory lock | Consumer group |
| Cleanup | N/A | `ZREMRANGEBYSCORE` per rule per sweep | Single `XTRIM MINID` |
| Complexity | Low | Medium | **Medium** (fewer moving parts than B) |

**Option A** doesn't solve the performance problems. **Option B** needs N Valkey
writes per matching sample (one `ZADD` per rule) and N reads per sweep (one
`ZRANGEBYSCORE` per rule). **Option C** needs 1 write per matching sample
regardless of rule count, and a single blocking `XREADGROUP` for all rules.

### Recommendation: Option C — Valkey Streams

---

## Proposed architecture (Valkey Streams)

```
OTLP ingest ─► mpsc channel ─► flush_metrics() ─► Postgres (permanent storage)
  (HTTP)         (10K cap)        (every 1s)
                    │
                    ▼  (alert tap — every replica)
              AlertRouter (in-memory read-only rule index)
                    │
                    ▼  match metric_name + labels → relevant rule_ids
              XADD alert:samples * rules "id1,id2" ts {ms} v {value}
                    │
                    ▼  (Valkey Stream — shared, durable, ordered)
              ┌──────────────────────────────────────────┐
              │  alert:samples (single stream)           │
              │    entry: { rules, ts, v }               │
              │    auto-ID = ingest timestamp             │
              │    trimmed by XTRIM MINID                 │
              └──────────────────────────────────────────┘
                    ▲
                    │ XREADGROUP BLOCK (single consumer via consumer group)
              Alert evaluator (leader)
                    │
                    ├─ dispatch to in-memory RuleWindow ring buffers
                    ├─ evaluate immediately on each push
                    ├─ check_condition + next_alert_state
                    └─ fire_alert / resolve_alert → Postgres + event bus
                    │
              Sweep loop (every 10s, same task)
                    ├─ evict expired samples from ring buffers
                    ├─ evaluate "absent" conditions (no data = no push)
                    ├─ XTRIM stream
                    └─ heartbeat task registry
```

### Design details

#### 1. AlertRouter — lightweight rule index (every replica)

Every replica maintains a read-only index mapping metric names to rules. This
determines which ingested samples get written to the stream.

```rust
/// Read-only rule index, rebuilt on startup + rule change notification.
/// Lives on every replica. Does NOT hold samples or alert state.
pub struct AlertRouter {
    /// metric_name → Vec<(rule_id, labels_filter)>
    routes: HashMap<String, Vec<(Uuid, Option<serde_json::Value>)>>,
}

impl AlertRouter {
    /// Which rules care about this metric sample?
    fn matching_rules(&self, name: &str, labels: &serde_json::Value) -> Vec<Uuid> {
        self.routes.get(name).map_or_else(Vec::new, |rules| {
            rules.iter()
                .filter(|(_, filter)| match filter {
                    None => true,
                    Some(f) => json_contains(labels, f),
                })
                .map(|(id, _)| *id)
                .collect()
        })
    }

    /// Build from DB. Called on startup and on rule-change notification.
    async fn from_db(pool: &PgPool) -> Result<Self> {
        let rules = sqlx::query(
            "SELECT id, query FROM alert_rules WHERE enabled = true"
        ).fetch_all(pool).await?;

        let mut routes: HashMap<String, Vec<(Uuid, Option<Value>)>> = HashMap::new();
        for rule in &rules {
            let id: Uuid = rule.get("id");
            let query: String = rule.get("query");
            if let Ok(aq) = parse_alert_query(&query) {
                routes.entry(aq.metric_name)
                    .or_default()
                    .push((id, aq.labels));
            }
        }
        Ok(Self { routes })
    }
}
```

**Placement:** `Arc<RwLock<AlertRouter>>` on `AppState`.

**Only metrics matching active alert rules are written to the stream.** If no
rules reference `http_requests_total`, those samples never touch the stream.
When a new rule is created for a previously-untracked metric, the router is
rebuilt and samples start flowing from that moment. No backfill — the alert
window fills naturally over `window_secs`.

#### 2. Ingest tap — every replica XADDs to the stream

In `flush_metrics()` (batched, every 1s), after building records, route through
`AlertRouter` and write matching samples to the Valkey stream:

```rust
// In flush_metrics(), after draining the channel into `buffer`:
let router = state.alert_router.read().await;
for record in &buffer {
    let matching = router.matching_rules(&record.name, &record.labels);
    if matching.is_empty() {
        continue;
    }
    let rules_str = matching.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let ts_ms = record.timestamp.timestamp_millis().to_string();
    let value = record.value.to_string();

    // Best-effort — don't block metric flush if Valkey is slow
    let _ = state.valkey.xadd::<(), _, _, _, _>(
        "alert:samples",
        false,          // no NOMKSTREAM
        None,           // no cap (trimmed by sweep)
        "*",            // auto-generate ID
        [("r", rules_str.as_str()), ("t", ts_ms.as_str()), ("v", value.as_str())],
    ).await;
}
```

**One `XADD` per matching sample, regardless of how many rules match.** If a
sample matches 5 rules, the `r` field contains `"uuid1,uuid2,...,uuid5"`. The
leader parses this and dispatches to 5 ring buffers.

**Pipelining:** Since we're already inside `flush_metrics()` which batches up
to 500 records per 1s cycle, use `fred`'s pipeline to send all `XADD`s in a
single Valkey round-trip. This turns 500 individual network calls into 1:

```rust
let pipeline = state.valkey.next().pipeline();
for record in &buffer {
    let matching = router.matching_rules(&record.name, &record.labels);
    if matching.is_empty() {
        continue;
    }
    let rules_str = matching.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let ts_ms = record.timestamp.timestamp_millis().to_string();
    let value = record.value.to_string();
    let _ = pipeline.xadd::<(), _, _, _, _>(
        "alert:samples", false, None, "*",
        [("r", rules_str.as_str()), ("t", ts_ms.as_str()), ("v", value.as_str())],
    ).await;  // queued, not sent yet
}
let _ = pipeline.all::<()>().await;  // single round-trip
```

#### 3. Consumer group — built-in leader election

Valkey consumer groups handle leader election natively. Only one consumer in a
group receives each message. If a consumer dies, unacknowledged messages are
redelivered to another consumer.

```rust
// On startup (idempotent):
let _ = state.valkey.xgroup_create::<(), _, _, _>(
    "alert:samples",
    "alert_eval",   // group name
    "$",            // start from latest (no history replay)
    true,           // MKSTREAM — create stream if it doesn't exist
).await;
// Ignore error if group already exists (BUSYGROUP).
```

**Why `$` (latest) and not `0` (beginning)?** On first startup, there's no
stream history. On restart, the consumer group tracks the last acknowledged
message — `XREADGROUP ... >` automatically delivers only new messages.
Unacknowledged messages from a crashed leader are picked up via pending
entry recovery (see failover section).

#### 4. Alert evaluator — the leader task

```rust
async fn alert_evaluator_loop(
    state: AppState,
    cancel: CancellationToken,
) {
    state.task_registry.register("alert_evaluator", 30);

    // Consumer name = unique per replica (hostname or random ID)
    let consumer_name = hostname_or_random();

    // In-memory state
    let mut windows: HashMap<Uuid, RuleWindow> = HashMap::new();
    let mut rule_defs: HashMap<Uuid, RuleDef> = HashMap::new();
    load_rule_definitions(&state.pool, &mut rule_defs).await;

    // Recover any pending (unacknowledged) entries from a previous leader
    recover_pending(&state, &consumer_name, &mut windows, &rule_defs).await;

    let mut sweep_interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            () = cancel.cancelled() => break,

            // Primary path: blocking read from stream
            entries = xreadgroup_block(&state.valkey, &consumer_name) => {
                for (entry_id, fields) in entries {
                    process_entry(&state, &mut windows, &rule_defs, &fields).await;
                    // Acknowledge — entry won't be redelivered
                    let _ = state.valkey.xack::<(), _, _, _>(
                        "alert:samples", "alert_eval", entry_id,
                    ).await;
                }
                state.task_registry.heartbeat("alert_evaluator");
            }

            // Sweep: evict expired, check "absent", trim stream
            _ = sweep_interval.tick() => {
                let now = Utc::now();
                for window in windows.values_mut() {
                    window.evict_expired(now);
                }
                // "absent" conditions can only be detected by sweep
                // (no push = no trigger)
                evaluate_absent_rules(&state, &mut windows, &rule_defs).await;
                // Trim stream: keep max window + 1 min buffer
                trim_stream(&state).await;
                // Reload rule definitions (picks up changes)
                load_rule_definitions(&state.pool, &mut rule_defs).await;
                state.task_registry.heartbeat("alert_evaluator");
            }
        }
    }
}

async fn xreadgroup_block(
    valkey: &fred::clients::Pool,
    consumer: &str,
) -> Vec<(String, HashMap<String, String>)> {
    // BLOCK 5000 = wait up to 5s, then return empty (so select! can check cancel)
    // COUNT 100 = process up to 100 entries per batch
    let result = valkey.xreadgroup::<Value, _, _, _, _>(
        "alert_eval", consumer,
        Some(100),      // COUNT
        Some(5000),     // BLOCK ms
        false,          // NOACK
        "alert:samples",
        ">",            // only new messages
    ).await;
    // Parse result into Vec<(entry_id, fields)>
    parse_xreadgroup_result(result)
}
```

#### 5. Processing an entry

```rust
async fn process_entry(
    state: &AppState,
    windows: &mut HashMap<Uuid, RuleWindow>,
    rule_defs: &HashMap<Uuid, RuleDef>,
    fields: &HashMap<String, String>,
) {
    let Some(rules_str) = fields.get("r") else { return };
    let Some(ts_str) = fields.get("t") else { return };
    let Some(v_str) = fields.get("v") else { return };

    let Ok(ts_ms) = ts_str.parse::<i64>() else { return };
    let Some(ts) = DateTime::from_timestamp_millis(ts_ms) else { return };
    let Ok(value) = v_str.parse::<f64>() else { return };

    for rule_id_str in rules_str.split(',') {
        let Ok(rule_id) = rule_id_str.parse::<Uuid>() else { continue };
        let Some(def) = rule_defs.get(&rule_id) else { continue };

        let window = windows.entry(rule_id).or_insert_with(|| {
            RuleWindow::new(def)
        });

        window.push(ts, value);

        // Instant evaluation
        let aggregate = window.aggregate();
        let condition_met = check_condition(&def.condition, def.threshold, aggregate);
        let now = Utc::now();
        let transition = next_alert_state(
            &mut window.alert_state, condition_met, now, def.for_seconds,
        );

        if transition.should_fire {
            let _ = fire_alert(&state.pool, rule_id, aggregate).await;
            let _ = publish_alert_fired(state, def, aggregate).await;
        }
        if transition.should_resolve {
            let _ = resolve_alert(&state.pool, rule_id).await;
        }
    }
}
```

#### 6. RuleWindow — per-rule sliding window

```rust
struct RuleWindow {
    samples: VecDeque<(DateTime<Utc>, f64)>,
    window_secs: i32,
    aggregation: Aggregation,

    // Incremental aggregation state
    running_sum: f64,
    count: usize,

    // Alert state (replaces HashMap<Uuid, AlertState> from current loop)
    alert_state: AlertState,
}

impl RuleWindow {
    fn new(def: &RuleDef) -> Self {
        Self {
            samples: VecDeque::new(),
            window_secs: def.window_secs,
            aggregation: def.aggregation,
            running_sum: 0.0,
            count: 0,
            alert_state: AlertState {
                first_triggered: None,
                firing: false,
            },
        }
    }

    fn push(&mut self, ts: DateTime<Utc>, value: f64) {
        self.samples.push_back((ts, value));
        self.running_sum += value;
        self.count += 1;
    }

    fn evict_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = now - chrono::Duration::seconds(i64::from(self.window_secs));
        while let Some(&(ts, value)) = self.samples.front() {
            if ts < cutoff {
                self.samples.pop_front();
                self.running_sum -= value;
                self.count -= 1;
            } else {
                break;
            }
        }
    }

    fn aggregate(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        Some(match self.aggregation {
            Aggregation::Avg => self.running_sum / self.count as f64,
            Aggregation::Sum => self.running_sum,
            Aggregation::Count => self.count as f64,
            Aggregation::Max => self.samples.iter()
                .map(|(_, v)| *v)
                .fold(f64::NEG_INFINITY, f64::max),
            Aggregation::Min => self.samples.iter()
                .map(|(_, v)| *v)
                .fold(f64::INFINITY, f64::min),
        })
    }
}
```

**Memory per rule:** 16 bytes per sample (8 timestamp + 8 f64). Typical: 300s
window at 1 sample/sec = 300 entries = 4.8 KB. 500 rules = 2.4 MB.

#### 7. Failover — pending entry recovery

When a leader dies, unacknowledged entries stay in the consumer group's pending
list. The new leader claims them:

```rust
async fn recover_pending(
    state: &AppState,
    consumer: &str,
    windows: &mut HashMap<Uuid, RuleWindow>,
    rule_defs: &HashMap<Uuid, RuleDef>,
) {
    // XAUTOCLAIM: take ownership of entries pending > 30s (dead consumer)
    // This handles entries the previous leader read but didn't ACK.
    let result = state.valkey.xautoclaim::<Value, _, _, _>(
        "alert:samples",
        "alert_eval",
        consumer,
        30_000, // min idle time ms — claim entries idle > 30s
        "0-0",  // start from beginning of pending list
        Some(500),
    ).await;

    // Process each recovered entry and ACK it
    for (entry_id, fields) in parse_autoclaim_result(result) {
        process_entry(state, windows, rule_defs, &fields).await;
        let _ = state.valkey.xack::<(), _, _, _>(
            "alert:samples", "alert_eval", entry_id,
        ).await;
    }
}
```

**On failover, the new leader:**
1. Claims pending entries from the dead consumer (`XAUTOCLAIM`)
2. Processes them into ring buffers
3. Continues reading new entries with `XREADGROUP ... >`
4. Ring buffers start empty but fill naturally — alerts resume evaluating as
   data accumulates

**Alert state (`first_triggered`, `firing`) is lost on failover.** This means
a firing alert may re-trigger after `for_seconds` elapses again. Acceptable —
`for_seconds` is typically 60–300s. If this becomes a problem, persist alert
state to Valkey hashes (see "Future improvements").

#### 8. Rule sync across replicas

When a rule is created/updated/deleted, the handling replica:

1. Updates `alert_rules` in Postgres (as today).
2. Publishes a notification: `PUBLISH alert:rules:changed ""`.
3. All replicas subscribe to this channel and rebuild their `AlertRouter`.

The evaluator leader also reloads `rule_defs` on the sweep interval (every
10s). Combined with the pub/sub notification, rule changes take effect within
seconds.

```rust
// Background task on every replica:
async fn alert_rule_subscriber(state: AppState, cancel: CancellationToken) {
    let subscriber = state.valkey.next().clone_new();
    let _ = subscriber.init().await;
    let _ = subscriber.subscribe("alert:rules:changed").await;
    let mut message_rx = subscriber.message_rx();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = subscriber.unsubscribe("alert:rules:changed").await;
                break;
            }
            msg = message_rx.recv() => {
                if msg.is_some() {
                    if let Ok(router) = AlertRouter::from_db(&state.pool).await {
                        *state.alert_router.write().await = router;
                    }
                }
            }
        }
    }
}
```

**New rules for previously-untracked metrics:** Once the `AlertRouter` is
rebuilt, samples for the new metric start flowing into the stream. The ring
buffer fills naturally over `window_secs`. No backfill — the alert evaluates
with whatever data has accumulated. This is correct: a rule created just now
has no historical context, and the DB wouldn't have relevant data in the stream
anyway.

#### 9. Stream trimming

The sweep loop trims old entries to bound memory:

```rust
async fn trim_stream(state: &AppState) {
    let max_window_secs = state.config.alert_max_window_secs; // default 86400
    let cutoff_ms = Utc::now().timestamp_millis()
        - (i64::from(max_window_secs) * 1000)
        - 60_000; // 1 min buffer
    let min_id = format!("{cutoff_ms}-0");

    let _ = state.valkey.xtrim::<(), _>(
        "alert:samples",
        XCap::MinID { approximate: true, id: min_id.into() },
    ).await;
}
```

In practice the stream only needs to hold entries as old as the longest active
rule's window. With the default max of 86400s (24h), entries older than ~24h
are trimmed.

---

## What changes where

| File | Change |
|---|---|
| `src/observe/alert.rs` | Add `AlertRouter`, `RuleWindow`, `alert_evaluator_loop` (stream consumer), `alert_rule_subscriber`, `process_entry()`, `recover_pending()`, `trim_stream()`. Keep `evaluate_metric()` for API. Deprecate `evaluate_alerts_loop`. |
| `src/observe/ingest.rs` | In `flush_metrics()`: after drain, check `AlertRouter` and `XADD` matching samples to `alert:samples`. |
| `src/observe/mod.rs` | Spawn `alert_evaluator_loop` + `alert_rule_subscriber`. Remove old `evaluate_alerts_loop`. |
| `src/store.rs` | Add `alert_router: Arc<RwLock<AlertRouter>>` to `AppState`. |
| `src/config.rs` | Add `PLATFORM_ALERT_MAX_WINDOW_SECS` (default 86400). |
| `src/observe/alert.rs` (CRUD) | After create/update/delete: `PUBLISH alert:rules:changed`. |

---

## Comparison with current approach

| Aspect | Current (DB poll) | Proposed (Valkey Stream) |
|---|---|---|
| Alert latency | 30–60s | < 100ms (push-based) |
| DB queries per eval cycle | N (one per rule) | 0 (reads from stream + in-memory) |
| Valkey writes per matching sample | 0 | 1 (`XADD`) |
| Scales with | Rule count × poll freq | Ingest throughput (stream routing) |
| "absent" detection | Every 30s | Every 10s (sweep) |
| Memory (Valkey) | ~0 | Stream entries (~50 bytes each) |
| Memory (process) | `HashMap<Uuid, AlertState>` | Ring buffers (~2.4 MB for 500 rules) |
| Multi-replica correctness | Duplicate alerts | Correct (consumer group = single consumer) |
| Leader failover | State lost, resets `for_seconds` | Pending entries recovered; state lost but ring buffers refill from stream |
| Code complexity | Simple loop | Moderate (stream consumer + router + sweep) |
| New rule for untracked metric | DB has all historical data | Data accumulates from rule creation onward |

---

## Migration path

### Phase A — Add advisory lock to existing loop (immediate fix)

Add `pg_try_advisory_lock` to current `evaluate_alerts_loop`. Only leader
evaluates. Fixes duplicate alerts with 2+ replicas. One small change, zero risk.

### Phase B — Shadow mode: stream consumer alongside DB poll

1. Add `AlertRouter` + ingest `XADD` tap.
2. Add `alert_evaluator_loop` (stream consumer) — shadow mode: logs evaluation
   results but does NOT fire alerts.
3. Compare shadow results with DB-poll results each cycle. Log mismatches.
4. Add `alert_rule_subscriber` for rule sync.

### Phase C — Switch to stream evaluation

1. Stream consumer takes over firing/resolving.
2. DB-poll loop disabled (behind `PLATFORM_ALERT_POLL_FALLBACK=true`).
3. Alert state moves to in-memory ring buffers.

### Phase D — Cleanup

1. Remove poll fallback.
2. Remove shadow comparison metrics.
3. `evaluate_metric()` stays as utility for ad-hoc API metric queries.

---

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| **Valkey unavailable** — can't read stream | Fall back to DB poll (`evaluate_metric()`). Evaluator detects Valkey errors and logs warning. Phase A advisory lock keeps working. |
| **XADD on ingest hot path** | Best-effort (`let _ =`). If Valkey is slow, samples are dropped from stream but still persist to Postgres. Alerting degrades gracefully. |
| **Stream memory** | `XTRIM MINID` in sweep. Max stream size = ingest rate × max window. At 1K matching samples/sec × 86400s = 86M entries × ~50 bytes ≈ 4.3 GB. In practice, far less — most rules have 5-min windows and not all samples match rules. |
| **Consumer group only has one active consumer** | By design — we want single-evaluator. If we later need parallel evaluation, consumer groups support multiple consumers with entry partitioning. |
| **Alert state lost on failover** | `for_seconds` hold-off resets. Acceptable for typical values (60–300s). Can persist to Valkey hashes later if needed. |
| **New rule for untracked metric has no data** | By design — alert window fills naturally from creation time. Same as current DB-poll behavior for a brand new rule + metric. |
| **Stale AlertRouter** | Pub/sub notification + periodic reload in sweep (every 10s). Max delay: 10s for new rules to start streaming. |
| **Clock skew between replicas** | Timestamps come from the originating replica. Minor skew (< 1s) is acceptable for windows of 10s+. |
| **f64 precision drift in running_sum** | Over millions of add/subtract cycles, floating-point error can accumulate. Mitigation: periodically recompute sum from ring buffer (e.g., every 1000 operations). |

---

## Future improvements (not in scope)

- **Persist alert state to Valkey hashes** — survive failover without resetting
  `for_seconds`. Low priority since hold-off is typically short.
- **Per-rule evaluation frequency** — critical rules every 2s, warning rules
  every 30s. Currently all rules evaluate on every push.
- **Parallel consumer group** — partition rules across N consumers for very high
  rule counts. Valkey consumer groups support this natively.
