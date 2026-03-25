# Skill: Performance & Scalability Audit — Hot Paths, Resource Bounds & Bottlenecks

**Description:** Orchestrates 5 parallel AI agents that analyze the platform for performance bottlenecks, unbounded resource consumption, inefficient patterns, and scalability limits. Focuses on: N+1 queries, missing indexes, unbounded allocations, connection pool exhaustion, background task contention, and hot path inefficiency. The core question: *"What breaks first when load increases 10x?"*

**When to use:** Before a load test, before scaling to more users/projects, when observing slow responses, or when adding new background tasks.

---

## Orchestrator Instructions

You are the **Performance Auditor**. Your job is to:

1. Profile the codebase structure (file sizes, module complexity)
2. Launch 5 parallel agents analyzing different performance dimensions
3. Synthesize findings into a prioritized report
4. Produce a persistent `plans/perf-audit-<date>.md` report

### Severity Levels

| Severity | Meaning | Action |
|---|---|---|
| **CRITICAL** | OOM, connection exhaustion, or deadlock under normal load | Fix immediately |
| **HIGH** | O(N²) or worse on user-controlled N, N+1 queries on list endpoints | Fix before scaling |
| **MEDIUM** | Missing index, suboptimal allocation, unnecessary clone/copy | Fix when touching the area |
| **LOW** | Minor optimization opportunity, premature allocation | Fix only if measured |

---

## Phase 0: Baseline

```bash
# Largest source files (complexity proxy)
wc -l src/**/*.rs src/*.rs 2>/dev/null | sort -rn | head -20

# Count of async functions (concurrency surface)
grep -rn 'async fn' src/ --include='*.rs' | wc -l

# Background tasks
grep -rn 'tokio::spawn\|task::spawn' src/ --include='*.rs' | head -20

# Connection pools
grep -rn 'Pool\|pool' src/main.rs src/config.rs src/store/ --include='*.rs' | head -20

# Query count
grep -rn 'sqlx::query' src/ --include='*.rs' | wc -l
```

---

## Phase 1: Parallel Performance Audits

Launch **all 5 agents concurrently**.

---

### Agent 1: Database Query Performance

**Scope:** All `sqlx::query*!()` in `src/`, migration files (for index definitions)

**Read ALL query call sites and index definitions, then analyze:**

_N+1 query patterns:_
- [ ] List endpoints that return items with related data — do they JOIN or loop+fetch?
- [ ] Handlers that call DB in a loop (for each item in a list)
- [ ] Webhook dispatch: does `fire_webhooks()` query per-webhook or batch?
- [ ] Permission resolution: does checking permissions for N resources make N queries?

_Missing indexes:_
- [ ] For every `WHERE` clause: is there an index on the filtered column(s)?
- [ ] For every `ORDER BY`: is there an index supporting the sort?
- [ ] For every `JOIN ... ON`: is there an index on the FK column?
- [ ] For every `COUNT(*)`: can it use an index-only scan?
- [ ] Compound queries: does index column order match query column order?
- [ ] Partial indexes for soft-delete: `WHERE is_active = true`

_Unbounded queries:_
- [ ] Any query without `LIMIT`? (Could return entire table)
- [ ] Pagination: are `LIMIT` defaults enforced (max 100)?
- [ ] `IN (...)` clauses: is the list size bounded?
- [ ] Aggregate queries on growing tables: will they slow down over time?

_Query plan risks:_
- [ ] Sequential scans on large tables (issues, audit_log, agent_messages, observe data)
- [ ] `LIKE '%term%'` — requires sequential scan, consider trigram index or full-text search
- [ ] `DISTINCT` on large result sets
- [ ] Correlated subqueries

_Connection pool:_
- [ ] Max pool size vs expected concurrent connections
- [ ] Long-running queries that hold connections (transactions, large result sets)
- [ ] Connection timeout settings
- [ ] Pool exhaustion scenario: what happens when all connections are in use?

**Output:** Numbered findings with query location, estimated impact, and fix.

---

### Agent 2: Memory & Allocation Patterns

**Scope:** Entire `src/` — focus on request handlers, data processing, and streaming

**Scan for unbounded allocation patterns:**

_Unbounded Vec/String growth:_
- [ ] Handlers that collect all items into a `Vec` before responding (should stream/paginate)
- [ ] String concatenation in loops (should use `String::with_capacity` or `Write`)
- [ ] `collect::<Vec<_>>()` on unbounded iterators
- [ ] Response bodies built entirely in memory (large git diffs, log queries, blob downloads)

_Large payload handling:_
- [ ] Git pack receive: is it streamed or buffered entirely in memory?
- [ ] Registry blob upload: chunked streaming or full buffer?
- [ ] OTLP ingest: what happens with a 10MB trace payload?
- [ ] LFS upload: streamed to MinIO or buffered?
- [ ] File browsing: large file content — streamed or loaded entirely?

_Clone/Copy overhead:_
- [ ] Unnecessary `.clone()` on large data (AppState fields, config, response bodies)
- [ ] `String` where `&str` or `Arc<str>` would suffice
- [ ] `Vec<u8>` copies where `Bytes` would allow zero-copy

_Per-request allocations:_
- [ ] Objects created per-request that could be cached/pooled (regex, HTTP clients, K8s clients)
- [ ] Repeated JSON serialization of same data
- [ ] Template rendering that could be cached

_Memory leaks:_
- [ ] `Arc` cycles (two `Arc`s referencing each other)
- [ ] Growing `HashMap`/`Vec` in `AppState` that's never pruned
- [ ] Event listeners/subscriptions that aren't cleaned up
- [ ] Tokio tasks spawned without JoinHandle tracking

**Output:** Numbered findings with file:line, pattern description, and fix.

---

### Agent 3: Concurrency & Background Task Efficiency

**Scope:** `src/main.rs` (task spawning), `src/pipeline/executor.rs`, `src/deployer/reconciler.rs`, `src/observe/` (flush tasks), `src/agent/service.rs`, `src/store/eventbus.rs`, `src/auth/` (session cleanup)

**Analyze concurrent operation patterns:**

_Background task contention:_
- [ ] How many background tasks run simultaneously? List them all.
- [ ] Do any tasks compete for the same resources (DB connections, Valkey, K8s API)?
- [ ] Task scheduling: are intervals reasonable? (Too frequent = wasted CPU, too rare = stale data)
- [ ] Can a slow background task block other tasks? (Shared runtime, no dedicated runtime)

_Pipeline executor:_
- [ ] `PLATFORM_PIPELINE_MAX_PARALLEL` — is it enforced with a semaphore?
- [ ] What happens when max parallel is reached? Queue depth bounded?
- [ ] Pod polling: how often does it check pod status? Could it use watch instead?
- [ ] Log collection: does it stream or poll? Memory bounded?

_Deployer reconciler:_
- [ ] Reconciliation interval: how often? Is it configurable?
- [ ] Does it reconcile ALL targets every cycle or only changed ones?
- [ ] K8s API call volume: O(targets) or O(targets × resources)?
- [ ] Error retry: exponential backoff or thundering herd?

_Observe flush tasks:_
- [ ] Flush interval vs data volume: will flushes keep up?
- [ ] Memory buffer size between ingests: bounded?
- [ ] Parquet rotation: does it block ingestion?
- [ ] Alert evaluation: query volume per cycle

_Pub/sub efficiency:_
- [ ] Valkey pub/sub: one connection per session or shared?
- [ ] Message serialization overhead
- [ ] Channel fan-out: what happens with 100 active sessions?

_Session cleanup:_
- [ ] How often does cleanup run?
- [ ] Cleanup query efficiency (bulk delete or one-by-one?)
- [ ] K8s pod cleanup: does it batch API calls?

_Lock contention:_
- [ ] Any `Mutex`/`RwLock` in hot paths?
- [ ] Lock hold duration: are locks held across async boundaries? (Can cause deadlock)
- [ ] Can `dashmap` or lock-free structures replace mutexes?

**Output:** Numbered findings with task identification, contention risk, and fix.

---

### Agent 4: HTTP Request Path Efficiency

**Scope:** `src/main.rs` (middleware stack), `src/api/` (all handlers), `src/auth/middleware.rs`, `src/ui.rs`

**Trace the hot path for common requests:**

_Middleware overhead:_
- [ ] How many middleware layers does every request traverse?
- [ ] Are there expensive operations in middleware that could be deferred?
- [ ] Security header middleware: is it zero-alloc for the common case?
- [ ] Body size limit: is it checked before reading the body?

_Auth overhead:_
- [ ] `AuthUser` extraction: how many DB/Valkey queries per request?
- [ ] Permission checking: cached? (Valkey) How often is cache hit vs miss?
- [ ] Permission cache serialization: JSON? MessagePack? Binary?

_Response serialization:_
- [ ] Large list responses: serialized entirely before sending?
- [ ] Could any responses benefit from streaming (SSE, chunked transfer)?
- [ ] JSON serialization: are there unnecessary intermediate `serde_json::Value` allocations?

_Static file serving:_
- [ ] rust-embed: are files served with `ETag`/`Last-Modified`?
- [ ] Are cache headers set? (Long cache for hashed assets, no-cache for HTML)
- [ ] gzip/brotli compression: enabled? At what layer?

_WebSocket efficiency:_
- [ ] Message framing overhead
- [ ] Backpressure: what if client is slow to consume messages?
- [ ] Connection limits: maximum concurrent WebSocket connections?

**Output:** Numbered findings with request path, estimated overhead, and fix.

---

### Agent 5: Scalability Limits & Bottleneck Identification

**Scope:** Cross-cutting analysis of entire `src/`, `helm/platform/values.yaml` (resource limits), `src/config.rs`

**Identify what breaks first at scale:**

_Scaling dimensions:_
- [ ] **Users**: What's the first bottleneck as user count grows? (Sessions? Permissions cache? Login rate limit?)
- [ ] **Projects**: What's the first bottleneck? (Git repo storage? Namespace count? RBAC query complexity?)
- [ ] **Active sessions**: What breaks? (Valkey pub/sub fan-out? Pod count? API connection pool?)
- [ ] **Pipeline runs**: What breaks? (Pod scheduling? Executor queue? Log storage?)
- [ ] **Observability data**: What breaks? (Parquet write throughput? Query latency? MinIO storage?)

_Resource limits:_
- [ ] Helm values: are CPU/memory limits reasonable for small/medium/large profiles?
- [ ] Postgres connection limit vs platform pool size vs concurrent users
- [ ] Valkey maxmemory vs expected cache size
- [ ] MinIO storage: is there a cleanup/retention policy?
- [ ] K8s namespace limit: how many namespaces before etcd slows down?

_Single points of failure:_
- [ ] Platform is a single replica — what happens during restart? (Rolling update? Downtime?)
- [ ] Git operations: are they the bottleneck? (Single process, file locking)
- [ ] Registry operations: are they the bottleneck?
- [ ] Valkey: single instance? What's the memory growth trajectory?

_Configuration tuning:_
- [ ] Connection pool sizes: too small (contention) or too large (resource waste)?
- [ ] Timeouts: are they appropriate for the expected latency?
- [ ] Buffer sizes: are they tuned for expected throughput?
- [ ] Background task intervals: are they appropriate for data volume?

**Output:** Numbered findings with scaling dimension, estimated limit, and fix.

---

## Phase 2: Synthesis

Deduplicate, prioritize, categorize:
- **OOM / Exhaustion risks** — unbounded allocations, connection pool exhaustion
- **Query performance** — N+1, missing indexes, sequential scans
- **Concurrency bottlenecks** — lock contention, task scheduling
- **Scalability limits** — what breaks first at 10x

Number findings P1, P2, P3... (P for Performance)

---

## Phase 3: Write Report

Persist as `plans/perf-audit-<YYYY-MM-DD>.md`.

Include:
- Executive summary with "what breaks first at 10x" answer
- Hot path analysis for the 5 most common request types
- Index coverage matrix
- Background task resource map
- Scaling limit table (dimension → bottleneck → estimated limit → fix)

---

## Phase 4: Summary to User

1. Performance health (one sentence)
2. Finding counts
3. "What breaks first at 10x" answer
4. Top 3 performance risks
5. Quick wins (easy fixes with biggest impact)
6. Path to report
