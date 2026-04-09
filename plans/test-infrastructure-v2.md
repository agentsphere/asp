# Plan: Test Infrastructure v2 — Logical Isolation & Tiered Execution

**Status**: Draft
**Created**: 2026-04-09
**Estimated LOC changed**: ~1,500 (helpers refactor + bootstrap batch + Justfile)

## Problem Statement

The platform has ~81K LOC of test code with 1,997 integration test functions across 64 binaries. A full integration run takes **~13-15 minutes wall clock** (2.5 min compilation + ~25s infra setup + ~10-12 min test execution). The actual test logic is fast (~50ms per test) but per-test setup overhead dominates.

### Measured Performance (from `test-ae635d52-output.txt`, 1997 tests @ 32 threads)

```
Compilation:           2m 32s
Infra setup:           ~25s  (namespace + pods + port-forward + RBAC)
Test execution:        ~10-12 min wall clock

Per-test timing distribution:
  0- 4s:    26 tests  (1.6%)
  5- 9s:   609 tests  (38.6%)   ← baseline after cache warm
 10-14s:   729 tests  (46.2%)   ← contention-inflated baseline
 15-19s:   135 tests  (8.6%)
 20-24s:    98 tests  (6.2%)    ← thundering herd (first 32 tests)
 25-39s:    54 tests  (3.4%)
 40-61s:    17 tests  (1.1%)    ← executor tests (real K8s pods)

Key stats:
  p50: 11.3s    avg: 12.7s
  First 32 tests: avg 21.0s  (thundering herd)
  Next  32 tests: avg  6.1s  (cache warm, less contention)
```

### Where the ~6-11s Per-Test Overhead Goes

**Critical nextest fact**: `cargo nextest` runs every test function in its **own isolated OS process**. Unlike `cargo test` (threads within a process), nextest gives zero shared memory between tests. This means static `OnceLock`/`OnceCell` cannot amortize connection setup across tests — every test process pays the full cost independently.

Every single test runs this setup before the first assertion:

| Operation | Time | What it does |
|-----------|------|-------------|
| `#[sqlx::test]` template copy | **3-4s** | `CREATE DATABASE test_xxx WITH TEMPLATE _sqlx_test_template` |
| `bootstrap::run()` | **1-1.5s** | 27 + 11 + ~350 individual INSERT queries + Argon2 hash |
| `registry::seed::seed_all()` | **0.3-0.5s** (cached) / **6-10s** (first) | Parse OCI tarballs → MinIO, insert metadata |
| Valkey + MinIO + Kube connections | **0.5-1s** | 3 TCP handshakes + TLS negotiation |
| Config + AppState construction | **0.2-0.3s** | 70+ config fields from env vars |
| **TOTAL** | **~6-7s** (steady) / **~21s** (first batch) | |

#### Why `#[sqlx::test]` is 3-4s

**Correction**: sqlx 0.8 does **NOT** use template databases. Each test runs `CREATE DATABASE {hash}` (empty), then runs all 69 migrations from scratch. The source (`sqlx-postgres-0.8.6/src/testing/mod.rs:170`) shows `create database {db_name}` with no `TEMPLATE` clause.

Additionally, sqlx creates a **master pool with `max_connections(20)`** shared via `OnceLock` — but since nextest runs 1 test per OS process, each process creates its own master pool. With 32 parallel processes × 20 master connections = **640 connections** attempted against Postgres. The per-test pool adds 5 more connections per process, totaling **800 potential connections**.

The 3-4s comes from:
1. 69 migrations running per test (no template reuse)
2. `CREATE DATABASE` + `pg_advisory_xact_lock` contention under 32 concurrent processes
3. Postgres default `fsync=on` forcing disk flushes on every migration statement

#### Why the first 32 tests take 21s

All 32 nextest processes start simultaneously and:
1. **One process** acquires the sqlx file lock, runs 69 migrations (~2-3s)
2. **31 processes** block on the lock, then all issue `CREATE DATABASE` at once (serialized by Postgres)
3. **All 32** race on registry seed file lock (one uploads blobs, 31 wait)
4. **All 32** establish Valkey + MinIO + Kube TCP connections

After the first batch: registry seed cache is warm, template DB exists, contention decreases → ~6-7s per test. But with 32 threads running continuously, contention keeps most tests in the 10-14s range.

### Slowest Test Groups (avg time per test)

| Binary | Count | Avg(s) | Max(s) | Bottleneck |
|--------|-------|--------|--------|-----------|
| executor_integration | 31 | 40.3 | 61.0 | Real K8s pods |
| mr_coverage_integration | 40 | 20.8 | 28.6 | Complex multi-step setup |
| admin_integration | 35 | 19.7 | 21.1 | Thundering herd (alphabetically first) |
| merge_gates_integration | 22 | 19.6 | 36.8 | Complex setup + git ops |
| git_browse_integration | 23 | 16.5 | 25.9 | Bare repo + working copy creation |
| git_smart_http_integration | 56 | 15.0 | 17.1 | Git repo setup per test |

### Additional Pain Points

1. **400+ lines of duplicated code** between `tests/helpers/mod.rs` (1,027 lines) and `tests/e2e_helpers/mod.rs` (1,138 lines). 18 functions copy-pasted identically.

2. **Every test pays for everything** — an auth test that only needs Postgres still waits for Kube client init, registry seed, MinIO operator construction.

3. **K8s namespace overhead (25s)** is paid even though 56 of 64 integration files never touch K8s.

## Current Architecture

```
                    just test-integration
                           │
                           ▼
                hack/test-in-cluster.sh
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
    Create K8s NS    Deploy pods      Setup RBAC
    (1s)          PG/Valkey/MinIO    (1s)
                  wait ready (15s)
                       │
                       ▼
             cargo nextest run (10-12 min)
             --test '*_integration'
             --test-threads 32
                       │
                ┌──────┴──────────────────────────────────────┐
                │  per test (each is its own OS process):     │
                │                                              │
                │  sqlx::test  ──→ CREATE DB WITH TEMPLATE     │ 3-4s
                │  bootstrap   ──→ 350+ individual INSERTs     │ 1-1.5s
                │  seed_all    ──→ OCI tarball → MinIO (cached)│ 0.3-0.5s
                │  connections ──→ Valkey + MinIO + Kube TCP    │ 0.5-1s
                │  config      ──→ 70+ env var reads           │ 0.2s
                └──────────────────────────────────────────────┘
```

### Isolation Already in Place

| Service | Isolation mechanism | Cross-test collision risk |
|---------|-------------------|--------------------------|
| **Postgres** | `#[sqlx::test]` creates ephemeral DB per test | Zero |
| **Valkey** | All keys UUID-scoped | Zero — no FLUSHDB, no shared keys |
| **MinIO** | `platform-e2e` bucket, UUID-scoped paths | Zero |
| **K8s** | Per-run namespace (`platform-test-{id}-*`) | Zero — but only 8/64 files need it |

**PG, Valkey, and MinIO are already logically isolated at the data level.** Deploying fresh instances per run is redundant for the 56 files that don't touch K8s.

### K8s Dependency Analysis (64 Integration Files)

**K8S_REQUIRED (8 files, 12.5%)** — actively create/query K8s resources:

| File | K8s Operations |
|------|---------------|
| `session_integration.rs` | Create/query namespaces, get pods |
| `registry_pull_integration.rs` | Create pods, wait for completion |
| `mesh_integration.rs` | Create namespaces, ConfigMaps, sync bundles |
| `gateway_controller_integration.rs` | Create Deployments, Services, namespaces |
| `executor_integration.rs` | Run pipeline steps as K8s pods |
| `executor_deploy_test_integration.rs` | Create namespaces, deploy manifests, test pods |
| `executor_coverage_integration.rs` | Run pipeline pods with various configs |
| `deployment_integration.rs` | Apply manifests, scale, patch via reconciler |

**NO K8S NEEDED (56 files, 87.5%)** — K8S_PASSIVE, INDIRECT, or no K8s at all.

## Proposed Solution

### A1. Tune Test Postgres for Speed (3-4s → <0.5s per test)

The 69 migrations run per test, and Postgres's crash-safety guarantees (fsync, WAL) add unnecessary disk I/O for ephemeral test data. Additionally, sqlx creates a master pool with 20 connections per process — with 32 parallel nextest processes, that's 640+ connections. Applied to `hack/test-manifests/postgres.yaml`:

```yaml
args:
  - "postgres"
  - "-c"
  - "max_connections=1000"        # 32 processes × (20 master + 5 per-test) = 800
  - "-c"
  - "fsync=off"                   # skip disk flush on every commit
  - "-c"
  - "synchronous_commit=off"      # don't wait for WAL write
  - "-c"
  - "full_page_writes=off"        # skip full-page images after checkpoint
  - "-c"
  - "shared_buffers=256MB"        # more RAM for concurrent DB creation
  - "-c"
  - "work_mem=64MB"               # faster sorts/hashes during migration
```

**Why this is safe**: Test databases are ephemeral — created and destroyed per test. A crash loses nothing.

**Expected impact**: Eliminates `PoolTimedOut` errors from connection exhaustion. Per-test migration time reduced by eliminating disk I/O. First-batch thundering herd reduced.

### A2. Batch Bootstrap Queries (350+ queries → ~5 queries)

`bootstrap::run()` currently does 350+ individual INSERT queries with individual network round-trips. Convert to multi-row INSERTs:

```rust
// Before: 27 individual INSERTs (1 per permission)
for perm in SYSTEM_PERMISSIONS {
    sqlx::query("INSERT INTO permissions ...").execute(pool).await?;
}

// After: 1 multi-row INSERT
sqlx::query(&format!(
    "INSERT INTO permissions (id, name, description) VALUES {} ON CONFLICT (name) DO NOTHING",
    SYSTEM_PERMISSIONS.iter().map(|p| format!("(gen_random_uuid(), '{}', '{}')", p.name, p.desc)).collect::<Vec<_>>().join(", ")
)).execute(pool).await?;

// Before: ~350 individual role_permission INSERTs
// After: 1 bulk INSERT from a VALUES list or CTE
```

**Expected impact**: 1-1.5s → ~50-100ms (350 network round-trips → ~5).

### A3. Lazy Registry Seed (skip for 90% of tests)

Currently `seed_all()` runs for every test. Make it opt-in:

```rust
pub async fn test_state(pool: PgPool) -> (AppState, String) {
    // ... existing setup, but WITHOUT seed_all() ...
}

pub async fn test_state_with_registry(pool: PgPool) -> (AppState, String) {
    let (state, token) = test_state(pool).await;
    platform::registry::seed::seed_all(&state.pool, &state.minio, &state.config.seed_images_path).await.ok();
    (state, token)
}
```

Only `registry_integration`, `registry_pull_integration`, and tests that exercise OCI image pull need to call `test_state_with_registry()`. The rest (~90%) skip the 0.3-0.5s seed cost entirely. More importantly, this eliminates the **6-10s first-process cache-warming penalty** for the majority of test binaries.

**Expected impact**: 0.3-0.5s → 0s for ~90% of tests. First-batch penalty eliminated for most binaries.

### A4. Optimize Connection Pool Sizes ~~(Share via OnceCell)~~

~~Share Valkey, MinIO, and Kube clients via `OnceLock`~~

**Correction**: nextest runs each test as a separate OS process. `OnceLock`/`OnceCell` cannot share state across processes. Each test pays its own connection cost.

Instead, optimize pool sizes to minimize per-process overhead:

- **Valkey**: Already pool size 1 (correct for per-process isolation)
- **PgPool from `#[sqlx::test]`**: sqlx creates this with defaults (likely `max_connections=5-10`). Since each test process runs a single test, it only needs 1-2 connections. Check if `#[sqlx::test]` accepts pool options to reduce this.
- **MinIO (opendal)**: Single operator, no pool tuning needed
- **Kube**: Single client, no pool tuning needed

**Expected impact**: Marginal (~50-100ms saved from faster pool init). The 0.5-1s connection cost is mostly TCP/TLS handshake, not pool negotiation.

### B. Tiered Execution (skip K8s overhead for 87.5% of tests)

#### B1. `just test-core` — No K8s Namespace (56 files)

Runs core integration tests directly against the long-lived services from `just cluster-up`, bypassing `test-in-cluster.sh` entirely:

```makefile
test-core filter="":
    #!/usr/bin/env bash
    set -euo pipefail
    source hack/cluster-info.sh

    # Kubeconfig for kube::Client::try_default() (kind cluster)
    export KUBECONFIG="${KUBECONFIG_PATH}"

    # Long-lived services from `just cluster-up`
    export DATABASE_URL="postgres://platform:dev@${NODE_IP}:5432/platform_dev?sslmode=require"
    export VALKEY_URL="redis://:dev@${NODE_IP}:6379"
    export MINIO_ENDPOINT="https://${NODE_IP}:9000"
    export MINIO_ACCESS_KEY="platform"
    export MINIO_SECRET_KEY="devdevdev"
    export MINIO_INSECURE=true
    export PLATFORM_MASTER_KEY="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    export PLATFORM_DEV=true
    export CLAUDE_CLI_PATH="${PWD}/cli/claude-mock/claude"
    export SQLX_OFFLINE=true

    # Exclude subsystem files
    cargo nextest run --test '*_integration' \
        -E 'not (
            binary(executor_integration) |
            binary(executor_deploy_test_integration) |
            binary(executor_coverage_integration) |
            binary(session_integration) |
            binary(registry_pull_integration) |
            binary(mesh_integration) |
            binary(gateway_controller_integration) |
            binary(deployment_integration)
        )' \
        --test-threads 32 --profile ci --no-fail-fast
```

**Key**: uses `NODE_IP` from `cluster-info.sh` to connect to the always-on cluster services. Same ports as `just cluster-up` provisions. No namespace creation, no pod deployment, no port-forwarding.

**Prerequisite**: `just cluster-up` must be running. Same as today.

#### B2. `just test-subsystem` — K8s Namespace (8 files)

```makefile
test-subsystem filter="":
    bash hack/test-in-cluster.sh \
        --expr 'binary(executor_integration) | binary(executor_deploy_test_integration) | binary(executor_coverage_integration) | binary(session_integration) | binary(registry_pull_integration) | binary(mesh_integration) | binary(gateway_controller_integration) | binary(deployment_integration)' \
        --threads 8
```

#### B3. Updated `just test-integration`

```makefile
test-integration:
    just test-core
    just test-subsystem
```

#### Kube Client for Core Tests

Core tests don't exercise K8s but `AppState` requires a `kube::Client`. Two options:

**Option A (simple)**: Keep `kube::Client::try_default()` — the kind cluster is always running, so this always succeeds. Cost: ~300ms per test for a client that's never used. Acceptable if Postgres tuning (A1) brings per-test time down sufficiently.

**Option B (strict)**: Use a stub kube client that panics on any API call. Catches accidental K8s usage in core tests. But contradicts the "no mock kube clients" preference from memory.

**Recommendation**: Option A. The 300ms is noise after A1-A3 optimizations reduce per-test time from 6-7s to ~1-2s. Keep it simple.

### C. Helper Consolidation

Extract shared code into `tests/common/`:

```
tests/
├── common/
│   ├── mod.rs          # Re-exports
│   ├── state.rs        # TestStateBuilder (unified test_state/e2e_state)
│   ├── http.rs         # get_json, post_json, etc. (18 duplicated functions)
│   ├── auth.rs         # admin_login, create_user, assign_role
│   ├── git.rs          # create_bare_repo, create_working_copy, git_cmd
│   ├── db.rs           # insert_mr, insert_pipeline, set_user_api_key
│   ├── k8s.rs          # wait_for_pod, pod_logs, cleanup_k8s
│   ├── polling.rs      # poll_pipeline_status, poll_deployment_status
│   └── server.rs       # start_test_server, start_pipeline_server
├── helpers/mod.rs      # → `pub use common::*;` (backward-compatible)
├── e2e_helpers/mod.rs  # → `pub use common::*;` (backward-compatible)
```

**`TestStateBuilder`** replaces two monolithic ~250-line functions:

```rust
pub struct TestStateBuilder {
    pool: PgPool,
    seed_registry: bool,       // default: false
    cli_spawn_enabled: bool,   // default: false
    minio_fallback: bool,      // default: false (e2e: true)
    platform_api_url: Option<String>,
}

// Backward-compatible:
pub async fn test_state(pool: PgPool) -> (AppState, String) {
    TestStateBuilder::new(pool).build().await
}
pub async fn e2e_state(pool: PgPool) -> (AppState, String) {
    TestStateBuilder::new(pool).with_minio_fallback().build().await
}
pub async fn test_state_with_registry(pool: PgPool) -> (AppState, String) {
    TestStateBuilder::new(pool).with_registry_seed().build().await
}
```

## Expected Impact Summary

| Metric | Before | After |
|--------|--------|-------|
| Per-test (steady state) | 6-7s | ~1-2s |
| Per-test (first batch) | 21s | ~3-5s |
| Full integration wall clock | ~13-15 min | ~5-7 min |
| `just test-core` (inner loop) | N/A | ~2-4 min (no infra wait) |
| `just test-subsystem` | N/A | ~2-3 min |
| Helper duplication | 400+ lines | 0 |

### Projected Per-Test Breakdown (after optimization)

| Operation | Before | After | How |
|-----------|--------|-------|-----|
| `CREATE DATABASE WITH TEMPLATE` | 3-4s | ~0.3s | Postgres tuning (fsync=off etc.) |
| Bootstrap seed | 1-1.5s | ~0.1s | Batched multi-row INSERTs |
| Registry seed | 0.3-0.5s | 0s | Lazy (opt-in only) |
| Connections (Valkey/MinIO/Kube) | 0.5-1s | 0.5-1s | Unchanged (nextest process isolation) |
| Config + State | 0.2-0.3s | 0.2s | No change |
| **TOTAL** | **6-7s** | **~1-2s** | |

## Implementation Order

```
Phase 1: Postgres tuning + batch bootstrap (biggest wins, lowest risk)
├── A1: Tune postgres.yaml          ← 1 YAML file change
└── A2: Batch bootstrap INSERTs     ← src/store/bootstrap.rs only

Phase 2: Lazy registry seed
└── A3: Make seed_all opt-in        ← helpers/mod.rs + ~5 test files

Phase 3: Tiered execution
├── B1: just test-core recipe       ← Justfile
├── B2: just test-subsystem recipe  ← Justfile
└── B3: Update test-integration     ← Justfile

Phase 4: Helper consolidation
└── C: Extract tests/common/        ← large refactor, all test files
```

**Phase 1 is a 30-minute change** (YAML + one Rust file) with the highest expected impact.

## Decisions Made

| Question | Decision | Rationale |
|----------|----------|-----------|
| Squash 69 migrations? | **No** | sqlx already uses template DB; bottleneck is `CREATE DATABASE` I/O, not migration count |
| Docker Compose for local services? | **No** | `just cluster-up` already provides PG/Valkey/MinIO. Two infra paths = maintenance burden |
| OnceCell for shared connections? | **No** | nextest runs each test as separate OS process; no shared memory |
| Mock kube client for core tests? | **No** | Real client from always-running kind cluster; 300ms cost acceptable |
| testcontainers-rs for K8s? | **Not now** | Kind is one-time cost, not per-run. Revisit if kind startup becomes bottleneck |

## Implementation Notes

### 1. Force `max_connections=1` on PgPool in test_state()

`cargo nextest` runs 32 parallel **processes**, each running a single test. The default `PgPool` from `#[sqlx::test]` may eagerly open 5-10 connections per process — with 32 processes that's 160-320 idle connections against a Postgres instance with `max_connections=300`. This wastes memory and handshake time for connections that are never used concurrently within a single test.

In `TestStateBuilder::build()` (or wherever the pool is reconfigured), ensure:
```rust
// The pool from #[sqlx::test] is already created, but if we create our own:
PgPoolOptions::new()
    .max_connections(1)  // single test = single connection is sufficient
    .connect(&database_url)
    .await?
```

Note: The pool is passed in from `#[sqlx::test]` macro, so we may not control its creation directly. If sqlx 0.8 doesn't expose pool options in the macro attribute, document this as a known limitation — the pool will use sqlx's internal defaults. The Postgres tuning (A1) handles the contention at the server side regardless.

### 2. KUBECONFIG resolution in `just test-core`

The `test-core` recipe runs tests **outside** `test-in-cluster.sh`, so `KUBECONFIG` won't be set by the script. `kube::Client::try_default()` will search `$HOME/.kube/config` or `KUBECONFIG` env var. Since the kind cluster writes its kubeconfig to `~/.kube/platform` (via `hack/cluster-up.sh`), the `test-core` recipe must explicitly export it:

```makefile
test-core:
    #!/usr/bin/env bash
    source hack/cluster-info.sh           # sets KUBECONFIG_PATH
    export KUBECONFIG="${KUBECONFIG_PATH}" # e.g. ~/.kube/platform
    # ... rest of env vars ...
    cargo nextest run ...
```

Without this, `try_default()` will either hang (trying wrong cluster) or panic (no kubeconfig found).

### 3. Database accumulation awareness

With 1,997 tests and `fsync=off`, Postgres will rapidly create and destroy ~2,000 databases per run. `#[sqlx::test]` handles cleanup — it drops each ephemeral database after the test function completes. However:

- Ensure `test_state()` doesn't hold extra PgPool references that prevent the `#[sqlx::test]` cleanup from dropping the DB. The `AppState.pool` is the same pool passed by the macro — when the test ends and the `AppState` is dropped, the pool closes, and sqlx drops the database.
- If tests panic (abort), the DB may leak. The `_sqlx_test_template` DB is intentionally persistent (reused across runs). Ephemeral test DBs that survive a crash are harmless — they'll be cleaned up on the next run or when the namespace is destroyed.
- Monitor Postgres with `SELECT count(*) FROM pg_database` during test runs to confirm DBs are being cleaned up and not accumulating beyond the active thread count (~32 at peak).

## Files to Modify

### Phase 1 (30 min):
- `hack/test-manifests/postgres.yaml` — add `fsync=off`, `synchronous_commit=off`, `full_page_writes=off`, `shared_buffers=256MB`, `work_mem=64MB`
- `src/store/bootstrap.rs` — batch 350+ individual INSERTs into ~5 multi-row INSERTs

### Phase 2 (1 hour):
- `tests/helpers/mod.rs` — remove `seed_all()` from `test_state()`, add `test_state_with_registry()`
- `tests/e2e_helpers/mod.rs` — same change
- ~5 test files that need registry — change to `test_state_with_registry()`

### Phase 3 (1 hour):
- `Justfile` — add `test-core`, `test-subsystem` recipes; update `test-integration`
- `.config/nextest.toml` — add subsystem test group
- `docs/testing.md` — document tiered execution

### Phase 4 (half day):
- New: `tests/common/{mod,state,http,auth,git,db,k8s,polling,server}.rs`
- Modified: `tests/helpers/mod.rs` → thin re-export wrapper
- Modified: `tests/e2e_helpers/mod.rs` → thin re-export wrapper
