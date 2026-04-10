# Plan: Separate OTLP Ingest + Platform Self-Instrumentation

## Context

The platform's OTLP ingest API (`/v1/traces`, `/v1/logs`, `/v1/metrics`) runs inside the main platform binary on the same HTTP server as all other APIs. This creates two problems:

1. **Platform can't fully observe itself** — it captures its own logs via `PlatformLogLayer` (service="platform"), but has NO HTTP request spans, NO RED metrics, NO process metrics. Deployed apps get all of this from the proxy wrapper, but the platform can't wrap itself with proxy because the OTLP endpoint IS itself.

2. **Platform restart = telemetry blackout** — when the platform restarts (update, crash, scaling), ALL OTLP ingestion stops. Proxy sidecars on deployed apps, infrastructure services, and pipelines lose their sink. Buffered data in proxy channels (up to 500 records per signal) is dropped. After restart, there's a gap in observability for the exact moment you need it most.

**Goal**: Make OTLP ingestion resilient to platform lifecycle events AND give the platform the same telemetry coverage as deployed apps.

---

## How It All Fits Together

```
                     ┌──────────────────────────────────────────────┐
                     │              Same binary: /platform          │
                     │                                              │
                     │  /platform              /platform --ingest   │
                     │  (full platform)        (OTLP-only mode)     │
                     │                                              │
                     │  ┌────────────────┐    ┌──────────────────┐  │
                     │  │ API, Git, UI,  │    │ /v1/traces       │  │
                     │  │ Deployer, ...  │    │ /v1/logs         │  │
                     │  │                │    │ /v1/metrics      │  │
                     │  │ Self-observe:  │    │ + flush tasks    │  │
                     │  │  spans+metrics ├───►│ + auth (tokens)  │  │
                     │  │  (direct write)│    │                  │  │
                     │  └────────────────┘    └────────▲─────────┘  │
                     └──────────────────────────────────┼───────────┘
                                                        │
                     ┌──────────────────────────────────┤
                     │    All proxy-wrapped services     │
                     │    point PLATFORM_API_URL at      │
                     │    the ingest process              │
                     │                                    │
                     │  ┌─────────┐  ┌─────────┐  ┌──────┘──┐
                     │  │ demo-app│  │postgres │  │ valkey  │ ...
                     │  │ +proxy  │  │ +proxy  │  │ +proxy  │
                     │  └─────────┘  └─────────┘  └─────────┘
```

**Key insight:** Same container image (`docker/Dockerfile`), same `/platform` binary — just different entrypoint args. No new crate, no code duplication.

- In **K8s**: Two Deployments from the same image. `platform` runs normally; `platform-ingest` runs with `--ingest`. Separate Services, separate rollout. Ingest stays up during platform updates.
- In **dev** (`just dev`): Two processes on the Mac. Ingest on port 4318 (background), main platform on its usual port (foreground). Same binary, `cargo run -- --ingest` vs `cargo run`.
- **OTLP endpoints stay in `src/observe/ingest.rs`** — no code moves. The `--ingest` mode just calls the existing `ingest::create_channels()` + `flush_*()` + `observe::ingest_router()` without starting anything else.

---

## Part 1: `--ingest` CLI Mode

Add a `--ingest` flag to `main()`. When set, run `run_ingest()` instead of the full platform.

**What `run_ingest()` starts:**
- Postgres connection pool (for `api_tokens` auth + span/log/metric writes)
- Valkey connection (for rate limiting + log live-tail pub/sub)
- OTLP router: `POST /v1/traces`, `POST /v1/logs`, `POST /v1/metrics`
- Flush background tasks: `flush_spans`, `flush_logs`, `flush_metrics`
- Auth middleware: Bearer token validation (reads `api_tokens` + `users` tables)
- Health endpoint: `GET /healthz`

**What it does NOT start:**
- No K8s client, no deployer, no pipeline executor, no agent orchestrator
- No git server, no registry, no UI, no MCP
- No WebAuthn, no mesh CA, no gateway controller
- No Parquet rotation, no alert evaluation, no k8s_watcher (those stay in main platform)

**AppState:** The `AuthUser` extractor expects `AppState`. Rather than refactoring the auth system, `run_ingest()` builds an `AppState` with only pool+valkey+config real, and cheap defaults for unused fields (dummy `Notify`, empty `RwLock`, etc.). No K8s client needed — skip `kube::Client::try_default()`. The ingest process never calls code that touches unused fields.

**Config:**
- `PLATFORM_INGEST_LISTEN` (default `0.0.0.0:4318` — OTLP HTTP standard port)
- Reuses existing `DATABASE_URL`, `VALKEY_URL` env vars
- Startup: <1s (just DB+Valkey connect + router)

### `run_ingest()` Skeleton

```rust
async fn run_ingest(listen: &str) -> anyhow::Result<()> {
    let cfg = config::Config::load();
    let pool = store::pool::connect(&cfg.database_url).await?;
    let valkey = store::valkey::connect(&cfg.valkey_url).await?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    // Spawn ONLY the flush tasks (no parquet, alerts, k8s watcher)
    let (channels, spans_rx, logs_rx, metrics_rx) = observe::ingest::create_channels();
    tokio::spawn(observe::ingest::flush_spans(pool.clone(), spans_rx, shutdown_rx.clone()));
    tokio::spawn(observe::ingest::flush_logs(pool.clone(), valkey.clone(), logs_rx, shutdown_rx.clone()));
    tokio::spawn(observe::ingest::flush_metrics(pool.clone(), metrics_rx, shutdown_rx.clone()));

    // Minimal AppState — only pool, valkey, config are real
    let state = build_ingest_state(pool, valkey, Arc::new(cfg));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(observe::ingest_router(channels))  // just the 3 OTLP routes
        .with_state(state);

    let addr: SocketAddr = listen.parse()?;
    tracing::info!(%addr, "starting OTLP ingest");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    let _ = shutdown_tx.send(());
    Ok(())
}
```

### New: `observe::ingest_router()`

```rust
// in src/observe/mod.rs — new function alongside existing router()
pub fn ingest_router(channels: ingest::IngestChannels) -> Router<AppState> {
    Router::new()
        .route("/v1/traces", axum::routing::post(ingest::ingest_traces))
        .route("/v1/logs", axum::routing::post(ingest::ingest_logs))
        .route("/v1/metrics", axum::routing::post(ingest::ingest_metrics))
        .layer(axum::Extension(channels))
    // No query or alert routes — those remain in the main platform only
}
```

---

## Part 2: Platform Self-Instrumentation

Extend the platform to write its own HTTP spans and process metrics directly into the observe pipeline — same pattern as `PlatformLogLayer` for logs.

### a) HTTP Request Spans

Extend `request_tracing_middleware` in `main.rs`:
- After the response, build a `SpanRecord` (method, path, status, duration_ms)
- Send via `IngestChannels.spans_tx.try_send()` (non-blocking, drop if full)
- `service: "platform"`, `kind: "server"`
- Parse incoming `traceparent` header → extract trace_id; generate new span_id
- No `traceparent` → generate new trace_id + span_id

The middleware gets `IngestChannels` via `axum::Extension` (same as OTLP handlers).

### b) Process Metrics

New background task in `src/observe/self_instrument.rs`:
- Sample RSS memory + CPU usage every 15s (matching proxy's interval)
- Use `sysinfo` crate (cross-platform: macOS + Linux)
- Write `MetricRecord` directly to `IngestChannels.metrics_tx`
- Metrics: `process.memory.rss` (gauge, bytes), `process.cpu.utilization` (gauge, millicores)
- Labels: `{"service": "platform"}`

### c) HTTP RED Metrics

Accumulate counters in `request_tracing_middleware`, flush every 15s:
- `http.server.request.count` (counter, `{request}`)
- `http.server.error.count` (counter, `{request}`, 5xx only)
- `http.server.duration` (histogram sum, `ms`)
- Histogram buckets: same as proxy (5, 10, 25, 50, 100, 250, 500, 1000, 5000, 10000 ms)

Use `AtomicU64` counters + `Mutex<Vec<f64>>` for durations → zero contention on the hot path.

---

## Part 3: K8s Deployment

### New manifest: `hack/test-manifests/platform-ingest.yaml`

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: platform-ingest
  labels:
    app: platform-ingest
spec:
  containers:
    - name: ingest
      command: ["/proxy/platform-proxy"]
      args: ["--wrap", "--", "/platform", "--ingest"]
      # (or just "/platform", "--ingest" if no proxy wrapping needed)
      env:
        - name: PLATFORM_INGEST_LISTEN
          value: "0.0.0.0:4318"
        - name: DATABASE_URL
          value: "postgres://platform:dev@postgres:5432/platform_dev"
        - name: VALKEY_URL
          value: "redis://:dev@valkey:6379"
      ports:
        - containerPort: 4318
      resources:
        requests: { cpu: 50m, memory: 64Mi }
        limits: { memory: 128Mi }
      livenessProbe:
        httpGet: { path: /healthz, port: 4318 }
      readinessProbe:
        httpGet: { path: /healthz, port: 4318 }
---
apiVersion: v1
kind: Service
metadata:
  name: platform-ingest
spec:
  selector:
    app: platform-ingest
  ports:
    - port: 4318
      targetPort: 4318
```

### Update OTLP target everywhere

All proxy-wrapped pods currently use `__PLATFORM_API_URL__` (sed-replaced by `deploy-services.sh`). Add a new placeholder `__PLATFORM_INGEST_URL__` or derive it from the existing URL:

- Proxy sidecars (in `src/deployer/applier.rs`): set `PLATFORM_API_URL` to `http://platform-ingest.{ns}.svc.cluster.local:4318` for OTLP export
- Infrastructure pods (postgres.yaml, valkey.yaml, minio.yaml): same
- Pipeline/agent pods: same

**Important:** The proxy sidecar uses `PLATFORM_API_URL` for BOTH the OTLP endpoint AND the mTLS cert bootstrap. We need to split these:
- `PLATFORM_API_URL` → stays as the main platform (for cert bootstrap, CLI auth, etc.)
- `PLATFORM_OTLP_URL` → new env var pointing at the ingest service (for OTLP export only)

This requires a small change in `crates/proxy/src/proxy/config.rs` to read `PLATFORM_OTLP_URL` with fallback to `PLATFORM_API_URL`.

---

## Part 4: Dev Mode

### `just dev` starts both processes

Update `Justfile`:
```makefile
dev-ingest:
    cargo run -- --ingest

dev:
    # Start ingest in background, main platform in foreground
    cargo run -- --ingest &
    cargo run
```

Or simpler: add `PLATFORM_INGEST_LISTEN` to `.env.dev` and let `dev-up.sh` generate it.

For the dev case, the main platform's self-instrumentation (Part 2) writes directly to its own observe pipeline (spans_tx/metrics_tx channels). It doesn't need the separate ingest process for its own telemetry — that's the in-process path. The separate ingest process is for infrastructure pods (postgres, valkey, minio) that need an external OTLP sink.

---

## Files to Modify

### Source code

| File | Change |
|------|--------|
| `src/main.rs` | Parse `--ingest` CLI arg → dispatch to `run_ingest()`. Extend `request_tracing_middleware` to build `SpanRecord` + accumulate RED counters. Add `IngestChannels` as `Extension`. |
| `src/observe/self_instrument.rs` | **New.** Process metrics sampler (sysinfo), RED metrics flusher, span builder helpers. |
| `src/observe/mod.rs` | Re-export `self_instrument`. Add `ingest_router()` (OTLP routes only, no query/alert). |
| `crates/proxy/src/proxy/config.rs` | Read `PLATFORM_OTLP_URL` with fallback to `PLATFORM_API_URL` for OTLP export target. |
| `crates/proxy/src/proxy/otlp.rs` | Use the new `otlp_url` config field instead of `api_url` for OTLP export. |
| `src/deployer/applier.rs` | Inject `PLATFORM_OTLP_URL` env var into proxy-wrapped containers, pointing at ingest service. |
| `Cargo.toml` | Add `sysinfo` dependency (for process metrics). |

### Deployment / infrastructure

| File | Change |
|------|--------|
| `hack/test-manifests/platform-ingest.yaml` | **New.** Ingest pod + service manifest. |
| `hack/deploy-services.sh` | Deploy `platform-ingest.yaml` alongside postgres/valkey/minio. Add `__PLATFORM_OTLP_URL__` sed replacement. |
| `hack/dev-up.sh` | Add `PLATFORM_INGEST_LISTEN` and `PLATFORM_OTLP_URL` to `.env.dev`. |
| `Justfile` | Add `dev-ingest` recipe. |

### Config

| Env Var | Default | Purpose |
|---------|---------|---------|
| `PLATFORM_INGEST_LISTEN` | `0.0.0.0:4318` | Listen address for `--ingest` mode |
| `PLATFORM_OTLP_URL` | (falls back to `PLATFORM_API_URL`) | OTLP export target for proxy sidecars |
| `PLATFORM_SELF_OBSERVE` | `true` | Enable/disable self-instrumentation (spans + metrics) |

---

## Implementation Order

1. **`observe::ingest_router()`** — extract OTLP-only router from existing `observe::router()` (5 min)
2. **`run_ingest()` + `--ingest` CLI** — minimal OTLP process with dummy AppState (30 min)
3. **Self-instrumentation: HTTP spans** — extend `request_tracing_middleware` (30 min)
4. **Self-instrumentation: process metrics** — `sysinfo` sampler background task (20 min)
5. **Self-instrumentation: RED metrics** — atomic counters + flush task (20 min)
6. **Proxy OTLP URL split** — `PLATFORM_OTLP_URL` in proxy config (10 min)
7. **K8s manifests** — ingest pod/service, deploy-services.sh update (15 min)
8. **Dev integration** — Justfile, dev-up.sh, .env.dev (10 min)
9. **Tests** — unit tests for self-instrument, integration test for ingest mode (30 min)

---

## Verification

1. **Unit tests**: self_instrument span builder, RED counter accumulation, process metrics record shape
2. **Integration test**: Start ingest router, POST OTLP traces/logs/metrics with Bearer auth, verify in Postgres
3. **Dev mode**: `just dev` + `just dev-ingest` in separate terminals; platform HTTP requests appear as spans in Observe UI; process metrics show up
4. **Resilience test**: Kill main platform process → verify OTLP ingest still accepts data → restart platform → verify no gap in ingested data
5. **K8s test**: Deploy platform-ingest pod, verify proxy sidecars successfully export to it

---

## What This Unlocks

- **Platform appears in Observe UI** with HTTP spans, process metrics, RED metrics, and logs — same telemetry as deployed apps
- **Zero telemetry gap** during platform updates/restarts — ingest process is independent
- **Infrastructure services** (postgres, valkey, minio) continue reporting during platform downtime
- **Deployed apps** never lose their OTLP sink
- **Foundation for proxy-wrapping platform** in K8s (optional future step for mTLS consistency)
