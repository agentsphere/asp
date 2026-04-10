# Production Readiness Audit — Full Findings

Comprehensive audit of the platform (~72K LOC, 15 modules) across 5 dimensions:
resilience, operations, architecture/decoupling, security, and performance. Run
2026-04-10 against commit `bcaf83c`.

The actionable subset (critical tier) is in `plans/production-readiness.md`.
This document preserves the complete findings for reference.

---

## CRITICAL — Must fix before production

### 1. No graceful shutdown / task supervision

- `src/main.rs:368-424` — All background tasks (`tokio::spawn`) are fire-and-forget
  with no `JoinHandle` tracking
- If a task panics (pipeline executor, observe flush, reconciler), it dies silently
  — no restart, no propagation
- On SIGTERM, `shutdown_tx.send(())` fires but the process doesn't wait for tasks
  to drain. Observe buffers (up to 30K records in memory) are lost
- 15+ spawned tasks, all JoinHandles immediately dropped
- Scenario: Pipeline executor panics due to malformed definition -> task dies
  silently -> 30+ pipelines queue in `pending` state indefinitely -> health
  endpoint shows "degraded" only after 30s+ -> users wait with no feedback

### 2. Single-instance only — no HA story

- `src/pipeline/executor.rs:84-93` — Polls `pipelines` table independently with
  no distributed lock. Two replicas will race to execute the same pipeline
- `src/deployer/reconciler.rs:83-98` — Same issue: competing reconcilers can apply
  conflicting K8s manifests
- No leader election, no advisory locks, no etcd integration
- Executor has partial mitigation: conditional `UPDATE ... WHERE status = 'pending'`
  (line 121-131), but race exists between SELECT and UPDATE
- Reconciler has no such mitigation — `reconcile_one()` begins work immediately

### 3. Connection pool starvation

- `src/store/pool.rs:12` — PgPool hardcoded to **20 connections**, not configurable.
  Pipeline executor + reconciler + API + background tasks easily exhaust this
- `src/store/valkey.rs:11` — Valkey pool hardcoded to **4 connections**. Every API
  request checks permissions via Valkey — bottleneck at ~50 concurrent users
- No `max_lifetime` on PG connections — stale connections after Postgres failover
  hang until 10s acquire timeout
- No env vars exposed for pool sizing
- sqlx does NOT hardcode limits — the 20/4 caps are entirely platform code

### 4. No request timeouts

- No global or per-route timeout middleware on the axum router (`main.rs:260-327`)
- A slow Postgres query or K8s API call blocks a handler indefinitely
- Only pipeline execution has a timeout (`pipeline_timeout_secs`)
- `tower 0.5` with `features = ["full"]` already in Cargo.toml — `TimeoutLayer`
  available but not used
- Health checks have 2s timeout, but no other operations do

---

## HIGH — Should fix for reliability

### 5. Observe data durability

- `src/observe/ingest.rs:27` — 10K-record bounded channels per signal type (traces,
  logs, metrics), flushed every 1s. Process crash = up to 1s of telemetry lost
- No write-ahead log or persistent journal
- `src/observe/parquet.rs:122-144` — Parquet upload + DB delete is not idempotent.
  Crash between MinIO write and DB delete = duplicate data on restart
- At ~1000 records/sec, crash loses 1000+ traces/logs
- Flush tasks do single best-effort drain on shutdown with no timeout — if DB is
  slow, drain hangs and blocks entire shutdown

### 6. OTLP ingest decoupling

- OTLP endpoints are well-structured internally (channel-based, async flush) —
  extracting to standalone binary would be straightforward
- Main coupling: shared PgPool and `create_channels()` in `src/observe/mod.rs:39-54`
- Separation lets projects keep ingesting telemetry when platform API is down for
  maintenance/upgrade
- Another agent is already planning this separation (`plans/otlp-ingest-separation.md`)

### 7. Git operations lack timeouts

- `smart_http.rs:500-545` — `receive_pack()`: `tokio::join!` on stdin/stdout +
  `child.wait()` — no timeout on any of these
- `smart_http.rs:698-740` — `run_git_service()` (upload-pack): `body.collect()`
  buffers entire request (up to 2 GB), `child.wait()` implicit — no timeout
- `git/browser.rs` already uses `tokio::time::timeout(GIT_TIMEOUT, ...)` with 30s
  — smart HTTP handlers never got the same treatment
- `git/ssh_server.rs:324` — SSH path also has no timeout on `child.wait()`
- Stalled git process blocks handler forever, consumes a connection + tokio task

### 8. Dev defaults unsafe for production

- `src/config.rs:216-217` — MinIO defaults to `platform`/`devdevdev` credentials
- No startup validation that warns/fails when production config is missing
- `PLATFORM_MASTER_KEY` is optional — secrets engine silently disables without it
  (line 105 in main.rs logs warning but doesn't fail)
- `PLATFORM_DEV=true` allows 50 auth attempts vs 5 in production (onboarding.rs:488)
- Dev mode generates random master key on each restart — secrets non-persistent
- Setup token expiry hardcoded to 1 hour (not configurable)

---

## MEDIUM — Important for scale and operations

### 9. Background task separation opportunities

- Pipeline executor, deployer reconciler, and OCI registry are candidates for
  extraction to separate binaries/crates
- Reduces blast radius (executor crash doesn't take down API) and enables
  independent scaling
- `crates/proxy/` already establishes the multi-crate pattern
- Single point of failure: if platform binary crashes, git pushes + CI pipelines +
  deployments + telemetry + notifications all break simultaneously

### 10. Rate limiting gaps

- Auth endpoints well-protected (login, passkeys, onboarding)
- OTLP ingest: 10K/min per project (recently improved)
- Mesh CSR signing: 100/hour per pod
- **Missing**: git push/pull, pipeline creation, project creation, agent session
  creation, deploy operations, API key operations, file uploads

### 11. Resource limit configurability

- Webhook concurrency hardcoded to 50 (`src/main.rs:204`) — not configurable
- Agent sessions hardcoded to 10 per user (`src/agent/service.rs`) — not configurable
- Observe buffer size hardcoded to 10K (`src/observe/ingest.rs:27`) — no env var
- CLI subprocesses configurable (default 10) via `PLATFORM_MAX_CLI_SUBPROCESSES`
- Pipeline max_parallel configurable (default 4) via `PLATFORM_PIPELINE_MAX_PARALLEL`
- No global limit across all pipelines — one user's large pipeline could starve others

### 12. Missing composite indexes

- No composite index on `pipelines(project_id, status, created_at)` for filtered
  list queries — currently uses `idx_pipelines_project` only
- As data grows, filtered pipeline/issue lists will degrade
- 64 indexes exist across schema, but filtered list queries not optimized

### 13. Registry chunk buffering

- `src/registry/blobs.rs:178,201` — `body: Bytes` extracts full chunk into memory,
  `body.to_vec()` copies it. 2 GB chunk = 4+ GB heap
- `blobs.rs:287-298` — `complete_upload()` reassembles all parts into `full_data:
  Vec<u8>` in memory. 5 GB blob = 5 GB heap + SHA256 pass
- `blobs.rs:97` — when `registry_proxy_blobs=true`, reads entire blob from MinIO
  into memory for GET response
- OpenDAL 0.55 has streaming Writer/Reader API — fix is straightforward

### 14. No gzip for UI assets

- `src/ui.rs` — Embedded SPA assets served without compression
- `rust-embed` has `compression` feature enabled in Cargo.toml, but response
  doesn't include `Content-Encoding: gzip` header
- 3-5x bandwidth savings available
- Cache headers correct: `max-age=86400` for assets, `no-cache` for `index.html`

### 15. Valkey key TTL gaps

- Registry upload sessions (`registry:upload:{id}`) set 1-hour TTL on update
  (blobs.rs:221) but initial creation may not set TTL
- Abandoned uploads after session update could leak keys
- All other Valkey keys (permissions, rate limits, passkey challenges) have proper TTLs

### 16. Database as bottleneck

- OTLP metric writes are not batched — high cardinality = N queries for N metrics
- Pipeline executor polls every 5s, reconciler every 10s — adds ~0.2 QPS per instance
- Observe data retention cleanup (`DELETE FROM {table} WHERE {col} < $1`) has no
  LIMIT clause — 1M+ row delete can lock entire table
- Parquet rotation runs hourly, could coincide with retention cleanup lock

### 17. Health check gaps

- `/healthz` is a hardcoded `"ok"` response with no dependency checks (line 261-262)
- Readiness check only verifies Postgres and Valkey, ignores MinIO and K8s API
- Background task stale detection takes 3x interval (30-90s for most tasks,
  1.5 hours for parquet rotation at 1800s interval)
- No liveness vs readiness distinction for K8s probes

### 18. Alert evaluator performance

- `src/observe/alert.rs` — Evaluates all alert rules sequentially
- No timeout on individual metric queries
- Evaluates every 30s — 100 rules = 100 sequential DB queries per cycle
- One slow query stalls all subsequent rules

---

## LOW — Nice to have

### 19. Session concurrency

- No limit on concurrent sessions per user
- No session enumeration/listing endpoint for users to see/revoke other sessions
- User deactivation properly deletes all sessions (admin.rs:944-948)

### 20. Secrets zeroize

- Decrypted secrets returned as `Vec<u8>` without zeroing from memory
- TODO comment in `secrets/engine.rs:75-77` acknowledges this
- Not actively logged, but could leak in core dumps/panics
- `zeroize` crate would add `Drop` impl to clear decrypted plaintext

### 21. Secret re-encryption

- After master key rotation, old ciphertext stays encrypted with previous key
- Dual-key decryption works (engine.rs:78-108) — tries current key, falls back
  to previous
- No background re-encryption job to migrate old ciphertext to new key
- Risk: previous key must be retained indefinitely or old secrets become
  unrecoverable

### 22. Backup/recovery

- No built-in backup tooling — entirely operator-managed
- Postgres: assumes external WAL archiving / pg_dump
- Git repos: filesystem at `/data/repos`, assumes operator snapshots
- MinIO: assumes operator-configured replication or S3 versioning
- Valkey: ephemeral cache, safe to lose on restart
- Observe data: `observe_retention_days` (default 30) auto-purges, no
  export/archival before purge

### 23. Self-observability gap

- Platform logs into its own observe pipeline (circular but intentional)
- `PLATFORM_SELF_OBSERVE_LEVEL` (default "warn") controls what's captured
- If observe module is broken, visibility into the platform itself is lost
- Cannot currently disable self-observability
- No correlation ID propagation across async task boundaries

### 24. TLS termination

- Platform does NOT handle TLS — binds to plain TCP (`main.rs:332`)
- Expects reverse proxy (Nginx/Traefik/Istio) to terminate TLS
- Service mesh mTLS is opt-in (`PLATFORM_MESH_ENABLED`, default false)
- `PLATFORM_SECURE_COOKIES` defaults to false — no warning if deployed with
  insecure cookies over HTTP
- No enforcement that platform must run behind HTTPS-terminating proxy

### 25. Multi-instance cache coherence

- `LazyLock<EntrypointCache>` in `deployer/reconciler.rs:22` — in-memory cache
  not shared across replicas
- Each pod instance has its own cache; cache misses cause repeated image inspections
- Not a correctness issue (cache is advisory), but wastes resources in HA setup

### 26. Dependency CVEs

- 3 known CVEs actively ignored in `deny.toml` with justifications:
  - RUSTSEC-2024-0370 (proc-macro-error): unmaintained, transitive via rust-embed
  - RUSTSEC-2024-0436 (paste): unmaintained, transitive from parquet
  - RUSTSEC-2023-0071 (rsa): timing side-channel, transitive from russh/ssh-key
- All low/medium risk with documented mitigations
- Git SSH uses Ed25519, not RSA — timing attack not exploitable

---

## Architecture — Decoupling Opportunities

### What could run independently (minimal refactoring)

| Component | Coupling | Effort | Benefit |
|-----------|----------|--------|---------|
| OTLP ingest | Low — shared PgPool only | 1 week | Telemetry survives platform downtime |
| OCI registry | Low — auth calls platform API | 1 week | Independent scaling for image pulls |
| Pipeline executor | Medium — shared AppState, direct DB | 2 weeks | Executor crash doesn't kill API |
| Deployer reconciler | Medium — shared AppState, K8s client | 2 weeks | Reconciler crash doesn't kill API |
| Git daemon (reads) | Medium — auth + repo path resolution | 2 weeks | Clone/fetch offloaded from async runtime |

### What's already well-decoupled

- Webhook delivery: `tokio::spawn` with semaphore, non-blocking to API
- Observe flush tasks: channel-based, independent lifecycles
- Proxy sidecar: separate binary in `crates/proxy/`

### Blast radius if platform binary crashes

| Component | Impact | Recovery |
|-----------|--------|----------|
| Git push/pull | Operations hang until restart | User retries |
| CI pipelines | Pending pipelines never execute | Until executor restarts |
| Deployments | Releases stuck in pending/progressing | Until reconciler restarts |
| OTLP ingest | Telemetry clients buffer locally, may lose data | Max ~10s buffer |
| Webhooks | Pending webhooks lost (no queue) | No recovery |
| API/Auth | All user requests fail | Until platform restarts |
| Registry | Image pulls fail | Until platform restarts |

---

## Security — Summary

| Area | Status | Key Finding |
|------|--------|-------------|
| Secret rotation | Good | Dual-key decryption, no auto re-encryption |
| Session management | Good | Proper invalidation, lacks concurrent session limits |
| API token security | Excellent | SHA-256 hashed, scoped, expiry enforced |
| CSRF protection | Excellent | SameSite=Strict + CSP + secure headers |
| Content Security Policy | Good | Restrictive default, documented preview exception |
| Dependency audit | Acceptable | 3 CVEs ignored with justification |
| K8s security contexts | Excellent | Non-root agents, all caps dropped, NetworkPolicies |
| Git authorization | Excellent | Force-push blocking, branch protection rules |
| Secrets in logs | Good | Tokens excluded, decrypted secrets lack zeroize |
| Input validation | Excellent | Comprehensive validation, no SQL/shell injection |

---

## Performance — Summary

| Issue | Severity | Location |
|-------|----------|----------|
| DB pool hardcoded to 20 | Critical | `src/store/pool.rs:12` |
| Valkey pool hardcoded to 4 | Critical | `src/store/valkey.rs:11` |
| Observe buffer 10K/channel | High | `src/observe/ingest.rs:27` |
| Registry chunk buffering 2 GB | High | `src/registry/blobs.rs:201` |
| Webhook concurrency hardcoded 50 | Medium | `src/main.rs:204` |
| Upload session TTL gap | Medium | `src/registry/blobs.rs` |
| No UI asset gzip header | Medium | `src/ui.rs` |
| Missing composite index on pipelines | Medium | migrations |
| Metric writes not batched | Medium | `src/observe/store.rs` |
| Retention DELETE without LIMIT | Medium | `src/observe/mod.rs` |
