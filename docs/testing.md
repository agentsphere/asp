# Testing Guide

This document covers all testing tiers for the platform: unit, integration, E2E, and LLM.

## Tier Boundary: Endpoint Scope vs User Journey

The boundary between integration and E2E is **how much of the user's reality are we simulating**, not whether the code is sync or async.

- **Integration** = Single API endpoint + ALL its side effects (sync and async). Can spawn background tasks, poll for pod status, wait for workers to complete. Tests: "does this endpoint work correctly, including everything it kicks off?"
- **E2E** = Multi-step user journeys spanning multiple API calls. Tests: "can a user complete this business workflow end-to-end?"

### Decision Tree

```
Does it touch I/O?
  No → Unit test
  Yes ↓
Is it testing a single endpoint and its side effects?
(even if those side effects are async — pods, executors, reconcilers, workers)
  Yes → Integration test
  No ↓
Is it a multi-step user journey across multiple endpoints?
  Yes ↓
Does it use a real Claude CLI with live Anthropic credentials?
  Yes → LLM test (just test-llm)
  No → E2E test (just test-e2e)
```

### Ambiguous Cases Resolved

| Scenario | Tier | Why |
|---|---|---|
| `create_session` → pod exists in K8s | Integration | Single endpoint, K8s API is a side effect |
| `create_session` → pod reaches `Running` → messages flow | Integration | Single endpoint + its async workers |
| Pipeline trigger → executor → pod completes → status success | Integration | Single endpoint + background executor |
| Webhook fires → wiremock receives POST | Integration | Single endpoint's async delivery side effect |
| Reconciler applies manifest after deployment create | Integration | Single endpoint + reconciler worker |
| Git push → commits readable via browse API | Integration | Single endpoint + filesystem side effect |
| Mock CLI emits canned NDJSON → session updated | Integration | Mock script, no real LLM |
| Login → create project → push → pipeline → deploy | E2E | Multi-step business journey |
| Create project → add agent → agent runs → results visible | E2E | Multi-step business journey |
| MR open → pipeline auto-triggers → merge → preview cleaned up | E2E | Multi-step business journey |

## Overview

| Tier | What's tested | Real infra | LLM | Command |
|---|---|---|---|---|
| Unit | Pure functions, state machines, parsers, validators | None | N/A | `just test-unit` |
| Integration | Single endpoint + all side effects (K8s pods, workers, mock CLI) | Postgres, Valkey, MinIO, K8s API | Mock (`CLAUDE_CLI_PATH`) | `just test-integration` |
| E2E | Multi-step business workflows across multiple endpoints | All real + background tasks | Disabled (`cli_spawn_enabled: false`) | `just test-e2e` |
| LLM | Claude CLI protocol with real Anthropic API | Real Claude CLI + credentials | Real | `just test-llm` |
| FE-BE | API contract + Playwright browser tests | dev cluster | N/A | `just test-integration` / `just types` / `just ui test` |

All tests use [cargo-nextest](https://nexte.st/) as the test runner.

**Coverage target**: 100% on unit + integration (diff-only enforcement via `just cov-diff-check`). E2E covers critical user journeys only.

**LLM mocking strategy**: No real LLM calls in unit/integration/E2E. Integration tests use `CLAUDE_CLI_PATH=cli/claude-mock/claude` (set automatically by `test_state()`). The mock script exits instantly with canned NDJSON. Separate `just test-llm` for real Anthropic API protocol tests.

**Frontend-Backend integration testing** is covered in a dedicated guide: [`docs/fe-be-testing.md`](fe-be-testing.md). It describes three tiers that prevent type drift between the Rust API and the Preact UI: ts-rs auto-generated types, API contract integration tests, and Playwright browser E2E tests.

## Unit Tests

**Location**: inline `#[cfg(test)] mod tests` blocks in source files under `src/`.

**What they cover**: business logic, parsers, state machines, permission resolution, encryption, validation, error mapping.

**Run**:
```bash
just test-unit          # cargo nextest run --lib
just test-doc           # cargo test --doc (doc examples)
```

**Conventions**:
- No I/O, no network, no filesystem — pure functions only.
- Use `#[test]` for sync tests, `#[tokio::test]` for async.
- Mock dependencies with in-memory structs (no external crate needed).
- `proptest` for parser/serialization round-trips.
- Keep test modules at the bottom of each source file.

**Examples of well-tested modules**:
- `src/pipeline/definition.rs` — YAML parsing, trigger matching, branch pattern matching
- `src/rbac/types.rs` — permission round-trips, serde, display
- `src/rbac/resolver.rs` — cache keys, scope matching
- `src/validation.rs` — input validation, SSRF detection, container image checks
- `src/secrets/engine.rs` — AES-256-GCM encrypt/decrypt round-trips
- `src/observe/proto.rs` — OTLP protobuf encoding/decoding, severity/span-kind mapping

## Integration Tests

**Location**: `tests/*_integration.rs` (52 files).

**What they cover**: Single API endpoint + all its side effects against real infrastructure. Each test targets one endpoint and verifies its complete behavior, including async side effects like background workers, K8s pod creation, mock CLI subprocess execution, webhook delivery, and reconciler runs.

**Integration tests can be async.** If a handler kicks off a background task (executor, reconciler, pod), the integration test spawns that task and polls/waits for the outcome. The test is still "integration" because it validates one endpoint's complete behavior — the async worker is an implementation detail.

**Mock CLI in integration tests**: `test_state()` always sets `CLAUDE_CLI_PATH` to `cli/claude-mock/claude`. Tests that need the CLI subprocess path set `cli_spawn_enabled: true` via `test_state_with_cli(pool, true)`. The mock script exits instantly with canned NDJSON — no external dependency, no runtime cost.

**Run**:
```bash
just test-integration   # ephemeral services in cluster, auto port-forward
```

### How it works

Integration tests run via `hack/test-in-cluster.sh`, which automates the entire lifecycle:

1. **Creates a fresh K8s namespace** (`test-{timestamp}-{random}`) in the dev cluster
2. **Deploys lightweight service pods** — Postgres, Valkey, MinIO (~5s to ready)
3. **Finds free local ports** dynamically (no port conflicts)
4. **Port-forwards** from cluster services to localhost
5. **Runs `cargo nextest run`** natively with env vars pointing to the forwarded ports
6. **Cleans up** the namespace on exit (via `trap` on EXIT/INT/TERM)

This means each test run gets fully isolated services with zero chance of cross-run pollution, and no fixed port requirements.

### Prerequisites

A dev cluster must be running:

```bash
just cluster-up    # one-time setup
```

No manual port-forwarding, database creation, or migration is needed — the script handles everything. The `platform` Postgres user is a superuser in the test namespace, so `#[sqlx::test]` can create ephemeral databases automatically.

### Running specific tests

```bash
# All integration tests (default)
just test-integration

# Custom parallelism
bash hack/test-in-cluster.sh --filter '*_integration' --threads 8

# Single test file
bash hack/test-in-cluster.sh --filter 'auth_integration'
```

### Key pattern — `#[sqlx::test]`

```rust
#[sqlx::test(migrations = "./migrations")]
async fn create_and_fetch_user(pool: PgPool) {
    // pool is a fresh database with all migrations applied
    // automatically cleaned up after test
}
```

### Integration test files

- `admin_integration.rs` — admin user/role management
- `alert_eval_integration.rs` — alert evaluation logic
- `auth_integration.rs` — login, tokens, sessions, password hashing
- `contract_integration.rs` — FE-BE API contract tests
- `create_app_integration.rs` — app/bot session creation
- `dashboard_integration.rs` — dashboard/onboarding status
- `deployment_integration.rs` — deployment CRUD, status, rollback, preview lifecycle
- `eventbus_integration.rs` — event bus handlers
- `git_browse_integration.rs` — git browse APIs (branches, tree, blob, commits)
- `git_smart_http_integration.rs` — git smart HTTP protocol, LFS
- `issue_mr_integration.rs` — issues, comments, merge requests, reviews
- `notification_integration.rs` — notification creation, queries
- `observe_ingest_integration.rs` — OTLP ingest endpoints
- `observe_integration.rs` — observability query, alerts, metrics
- `passkey_integration.rs` — WebAuthn/passkey flows
- `pipeline_integration.rs` — pipeline CRUD, cancel, artifacts
- `pipeline_trigger_integration.rs` — pipeline trigger logic (push, MR, API)
- `project_integration.rs` — project CRUD, soft-delete, visibility
- `rbac_integration.rs` — role assignment, permission resolution, delegation
- `registry_integration.rs` — container registry push/pull, GC
- `secrets_integration.rs` — secrets CRUD, user keys
- `session_integration.rs` — agent session management, spawn lineage
- `user_keys_integration.rs` — user API key management
- `webhook_integration.rs` — webhook CRUD, dispatch, HMAC signing, concurrency
- `workspace_integration.rs` — workspace CRUD, membership

### Test helpers

All shared helpers are in `tests/helpers/mod.rs`:

**State & Router**:
- `test_state(pool: PgPool) -> (AppState, String)` — builds full state with real Valkey, MinIO, K8s client. Returns `(state, admin_token)`. The admin API token is created directly in the DB, bypassing the login endpoint's rate limiter. No FLUSHDB — all Valkey keys are UUID-scoped and never collide between parallel tests.
- `test_state_with_cli(pool, cli_spawn_enabled) -> (AppState, String)` — wraps `test_state()`, always sets `CLAUDE_CLI_PATH` to the mock CLI script. Pass `true` to enable CLI subprocess spawning for tests that exercise the mock CLI flow.
- `test_router(state: AppState) -> Router` — merges API + observe + registry routers with state.
- `start_test_server(pool) -> (AppState, String, JoinHandle)` — real TCP server for tests needing pod connectivity (binds to `PLATFORM_LISTEN_PORT`).

**Auth**:
- `admin_login(&app) -> String` — login as bootstrap admin via POST `/api/auth/login`, returns bearer token. **Only use for tests that specifically test login/session behavior** (~2 tests in `auth_integration.rs`). All other tests should use the pre-created `admin_token` from `test_state()`.
- `create_user(&app, admin_token, name, email) -> (Uuid, String)` — create user + login.
- `assign_role(&app, admin_token, user_id, role_name, project_id, &pool)` — assign role.

**HTTP**:
- `get_json`, `post_json`, `patch_json`, `put_json`, `delete_json` — HTTP helpers with bearer auth.
- `get_bytes(&app, token, path) -> (StatusCode, Vec<u8>)` — GET raw bytes (for non-JSON endpoints).

**Git**:
- `create_bare_repo() -> (TempDir, PathBuf)` — bare git repo under `/tmp/platform-e2e/` (visible to cluster).
- `create_working_copy(&bare_path) -> (TempDir, PathBuf)` — clone + initial commit + push to main.
- `git_cmd(&dir, &[args]) -> String` — run git command, panic on failure.

**Important**: The admin token from `test_state()` bypasses the login rate limiter entirely. This avoids the `rate:login:admin` Valkey key collision that caused flaky tests when hundreds of parallel tests all called `admin_login()`. The rate limit key was the only cross-test Valkey key — all other keys (permission cache, upload sessions, WebAuthn challenges) contain per-test UUIDs.

## E2E Tests

**Location**: `tests/e2e_*.rs` (9 files) + `tests/e2e_helpers/mod.rs`.

**What they cover**: multi-step user journeys spanning multiple API calls. E2E tests simulate real business workflows (pipeline execution, git protocol, deployment reconciliation, agent pod lifecycle) rather than individual endpoint behavior.

### Prerequisites

A dev cluster with all services running. One-time setup:

```bash
just cluster-up
```

This creates the dev cluster with shared mount, Postgres, Valkey, MinIO, namespaces, and buckets. See [Cluster Management](#cluster-management) for details.

### Running E2E Tests

```bash
# All E2E tests (ephemeral namespace, auto port-forward)
just test-e2e

# Specific test file
bash hack/test-in-cluster.sh --type e2e --filter 'e2e_pipeline'

# Custom parallelism
bash hack/test-in-cluster.sh --type e2e --threads 1
```

For E2E tests, the script additionally creates `{namespace}-pipelines` and `{namespace}-agents` namespaces plus RBAC bindings so pipeline/agent pods can be created.

### E2E Test Architecture

Each E2E test:
1. Gets a fresh `PgPool` from `#[sqlx::test(migrations = "./migrations")]` (ephemeral DB)
2. Builds an `(AppState, admin_token)` with real K8s, MinIO, Valkey via `e2e_helpers::e2e_state(pool)`
3. Creates a test router via `e2e_helpers::test_router(state)`
4. Uses the pre-created `admin_token` (bypasses login rate limiter — same pattern as integration tests)
5. Exercises API endpoints using HTTP helpers (`get_json`, `post_json`, etc.)
6. Asserts on HTTP status codes and JSON response bodies

The test router is an in-memory axum `Router` — no TCP listener. Requests go through `tower::ServiceExt::oneshot()`.

### E2E Helper Functions

All helpers are in `tests/e2e_helpers/mod.rs`:

**State & Router**:
- `e2e_state(pool: PgPool) -> (AppState, String)` — builds full state with real services. MinIO bucket: `platform-e2e`. Reads pipeline/agent namespace from env vars (set by orchestration script). Returns `(state, admin_token)` — the admin API token is created directly in the DB, bypassing the login endpoint's rate limiter.
- `test_router(state: AppState) -> Router` — merges `platform::api::router()` with state.

**Auth**:
- `admin_login(&app) -> String` — login as bootstrap admin (password: `testpassword`), returns bearer token. **Only for tests that specifically test login/session behavior.** All other tests use the pre-created `admin_token`.
- `create_user(&app, admin_token, name, email) -> (Uuid, String)` — create user + login, returns (user_id, token).
- `assign_role(&app, admin_token, user_id, role_name, project_id, &pool)` — assign role to user.

**Git**:
- `create_bare_repo() -> (TempDir, PathBuf)` — bare git repo under `/tmp/platform-e2e/` (visible to cluster).
- `create_working_copy(&bare_path) -> (TempDir, PathBuf)` — clone + initial commit + push to main.
- `git_cmd(&dir, &[args]) -> String` — run git command, panic on failure.

**HTTP**:
- `get_json(&app, token, path) -> (StatusCode, Value)` — GET with bearer auth.
- `post_json(&app, token, path, body) -> (StatusCode, Value)` — POST JSON with bearer auth.
- `patch_json(&app, token, path, body) -> (StatusCode, Value)` — PATCH JSON.
- `delete_json(&app, token, path) -> (StatusCode, Value)` — DELETE.
- `get_bytes(&app, token, path) -> (StatusCode, Vec<u8>)` — GET raw bytes.

**K8s**:
- `wait_for_pod(&kube, namespace, name, timeout_secs) -> String` — poll until Succeeded/Failed.
- `cleanup_k8s(&kube, namespace, label)` — delete pods by label selector.
- `poll_pipeline_status(&app, token, project_id, pipeline_id, timeout_secs) -> String` — poll until terminal status.
- `poll_deployment_status(&app, token, project_id, env, expected, timeout_secs) -> String` — poll deployment.

### E2E Test Files

#### `e2e_pipeline.rs` (10 tests)

Tests pipeline triggering, execution via real K8s pods, multi-step pipelines, cancellation, log capture, MinIO storage, and artifact upload/download.

**Critical patterns**:

1. **Spawn executor per test** — the test router does NOT include the background pipeline executor. Each test must spawn one:
   ```rust
   // Spawn the pipeline executor background task
   tokio::spawn(executor::run(state.clone()));
   // ... trigger pipeline ...
   state.pipeline_notify.notify_one();  // wake executor
   ```

2. **`.platform.yaml` format** — must have `pipeline:` top-level key:
   ```yaml
   pipeline:
     steps:
       - name: test
         image: alpine:3.19
         commands:
           - echo hello
   ```

3. **Git repo setup** — `setup_pipeline_project` creates a project, bare repo, working copy, commits `.platform.yaml`, and updates the project's `repo_path` in DB.

4. **Pod execution** — pipeline pods run in the ephemeral pipelines namespace. The executor creates pods with an init container (`alpine/git`) that clones the repo, then runs step commands. Repos must be under `/tmp/platform-e2e/` (shared mount).

Tests: `pipeline_trigger_and_execute`, `pipeline_with_multiple_steps`, `pipeline_step_failure`, `pipeline_cancel`, `step_logs_captured`, `step_logs_in_minio`, `artifact_upload_and_download`, `pipeline_branch_trigger_filter`, `pipeline_definition_parsing`, `concurrent_pipeline_limit`.

#### `e2e_git.rs` (3 tests)

Tests multi-step git protocol operations: smart HTTP push, clone, and merge request merge.

**Pattern**: create a bare repo + working copy, exercise multi-step git workflows. Single-endpoint browse tests (branches, tree, blob, commits) moved to `git_browse_integration.rs`.

Tests: `smart_http_push`, `smart_http_clone`, `merge_request_merge`.

#### `e2e_webhook.rs` (1 test)

Tests multi-step webhook dispatch: pipeline trigger → execution → completion → webhook fires.

Single-endpoint webhook dispatch tests (issue create fires webhook, HMAC signature, timeout, concurrency) moved to `webhook_integration.rs`.

Tests: `webhook_fires_on_pipeline_complete`.

#### `e2e_agent.rs` (8 tests)

Tests agent session lifecycle: creation, identity provisioning, pod spec generation, provider configuration, session stop, custom images, and log capture by the reaper.

**Pattern**: tests that need real K8s pods check if the kube client works and skip gracefully if not. Most tests verify API responses and DB state without requiring actual pod execution.

Tests: `agent_session_creation`, `agent_identity_created`, `agent_identity_cleanup`, `agent_pod_spec_correct`, `agent_role_determines_mcp_config`, `agent_session_stop`, `agent_session_with_custom_image`, `agent_reaper_captures_logs`.

#### `e2e_deployer.rs` (7 tests)

Tests deployer reconciler behavior with real K8s: basic deployment, rollback, stop (scale to zero), optimistic locking, multi-env reconciliation, history tracking, and preview TTL cleanup.

**Pattern**: Reconciler tests spawn a `ReconcilerGuard` (similar to `ExecutorGuard` for pipelines) that runs the reconciler loop in a background task. Deployment API CRUD tests and preview lifecycle tests moved to `deployment_integration.rs`.

Tests: `reconciler_deploys_basic_manifest`, `reconciler_rollback_restores_previous`, `reconciler_stop_scales_to_zero`, `reconciler_optimistic_lock`, `preview_expired_cleanup`, `reconciler_multi_env`, `reconciler_history_actions`.

## Ephemeral Test Infrastructure

Both integration and E2E tests use the same orchestration script (`hack/test-in-cluster.sh`) to provision isolated services per test run.

### How it works

```
┌─────────────────────────────────────────────────────────────┐
│  dev cluster (platform)                                    │
│                                                             │
│  ┌─ test-{timestamp}-{random} namespace ──────────────────┐ │
│  │                                                         │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐             │ │
│  │  │ Postgres │  │  Valkey  │  │  MinIO   │  pods       │ │
│  │  │ :5432    │  │  :6379   │  │  :9000   │             │ │
│  │  └────┬─────┘  └────┬─────┘  └────┬─────┘             │ │
│  └───────┼──────────────┼──────────────┼──────────────────┘ │
│          │              │              │                     │
│     port-forward   port-forward   port-forward              │
└──────────┼──────────────┼──────────────┼────────────────────┘
           │              │              │
     localhost:{free} localhost:{free} localhost:{free}
           │              │              │
     ┌─────┴──────────────┴──────────────┴─────┐
     │  cargo nextest run (native on host)      │
     │  DATABASE_URL, VALKEY_URL, MINIO_ENDPOINT│
     └──────────────────────────────────────────┘
```

**Key properties**:
- **No fixed ports** — dynamically finds free ports, so no conflicts with other services
- **Full isolation** — each run gets its own Postgres/Valkey/MinIO instances in a unique namespace
- **Auto cleanup** — namespace is deleted on exit, even on Ctrl+C or test failure
- **Native execution** — tests run as a normal `cargo nextest` process, so incremental compilation, IDE debugging, and coverage tools all work
- **Fast startup** — lightweight pods (alpine-based) are ready in ~5s

### Service pods

The K8s manifests live in `hack/test-manifests/`:

| Service | Image | Credentials | Readiness probe |
|---|---|---|---|
| Postgres | `postgres:16-alpine` | user: `platform`, pass: `dev`, db: `platform_dev` | `pg_isready -U platform` |
| Valkey | `valkey/valkey:8-alpine` | none | TCP :6379 |
| MinIO | `minio/minio:latest` | user: `platform`, pass: `devdevdev` | `/minio/health/live` |

The `platform` Postgres user is a superuser, so `#[sqlx::test]` can create/drop ephemeral test databases without additional grants.

### Environment variables

The script sets these before running `cargo nextest`:

| Variable | Value |
|---|---|
| `DATABASE_URL` | `postgres://platform:dev@127.0.0.1:{port}/platform_dev` |
| `VALKEY_URL` | `redis://127.0.0.1:{port}` |
| `MINIO_ENDPOINT` | `http://127.0.0.1:{port}` |
| `MINIO_ACCESS_KEY` | `platform` |
| `MINIO_SECRET_KEY` | `devdevdev` |
| `SQLX_OFFLINE` | `true` |
| `PLATFORM_MASTER_KEY` | test key (64 hex chars) |
| `PLATFORM_DEV` | `true` |
| `RUST_LOG` | `warn` |

For all test types, additionally:

| Variable | Value |
|---|---|
| `PLATFORM_PIPELINE_NAMESPACE` | `{namespace}-pipelines` |
| `PLATFORM_AGENT_NAMESPACE` | `{namespace}-agents` |
| `CLAUDE_CLI_PATH` | `{project_dir}/cli/claude-mock/claude` |

### Cleaning up stale namespaces

If a test run is killed without cleanup (e.g., `kill -9`), stale namespaces may remain:

```bash
just test-cleanup   # deletes all test-* namespaces
```

## Migration Status: E2E → Integration

Single-endpoint tests have been migrated from E2E to integration per the new boundary definition:

| Migrated from | Migrated to | Tests moved |
|---|---|---|
| `e2e_webhook.rs` | `webhook_integration.rs` | 5 (dispatch, HMAC, timeout, concurrency) |
| `e2e_git.rs` | `git_browse_integration.rs` | 5 (branches, tree, blob, commits, repo init) |
| `e2e_deployer.rs` | `deployment_integration.rs` | 2 (preview lifecycle, MR merge cleanup) |
| `agent_spawn_integration.rs` | `session_integration.rs` | 2 unique tests merged; 7 duplicates removed |
| `e2e_deployer.rs` | (deleted) | 8 duplicate CRUD tests removed |

**Remaining E2E tests** are all multi-step user journeys across 9 files.

**Future E2E tests** to be written as the suite matures:
- Onboarding journey: signup → create project → configure → first push → first pipeline
- Full agent workflow: create project → create session → agent completes task → logs visible
- MR lifecycle: branch → push → MR → pipeline → review → merge → preview cleanup
- Deployment pipeline: push → build → deploy → verify → rollback

## Common Pitfalls

1. **dev cluster not running** — `just test-integration` and `just test-e2e` require a dev cluster. Run `just cluster-up` first.

2. **Stale `.sqlx/` cache** — never use `sqlx::query!` macros in `tests/` files. Use dynamic `sqlx::query()` / `sqlx::query_as()` instead. The compile-time macros require the offline cache to be regenerated every time queries change.

3. **`/tmp/platform-e2e` mount** — pipeline pods use HostPath volumes to mount git repos. If repos are created in macOS temp dirs (`/var/folders/...`), they're invisible inside the cluster container. Always use `/tmp/platform-e2e/` as the base path (the helpers do this automatically).

4. **KUBECONFIG path** — in sandboxed environments `$HOME` may resolve to `/`. The script uses `$HOME/.kube/platform`. If running manually, use the full path: `KUBECONFIG=/Users/<you>/.kube/platform`.

5. **Pipeline executor not running** — the test router does NOT spawn background tasks. Pipeline E2E tests must spawn the executor via `tokio::spawn(executor::run(state.clone()))` and call `state.pipeline_notify.notify_one()` after triggering.

6. **SSRF blocking localhost** — webhook tests can't register `http://127.0.0.1:*` URLs via the API. Insert directly into DB.

7. **Race conditions** — after triggering a pipeline, the executor may pick it up before your next assertion. Don't assert `status == "pending"` immediately after trigger — use `poll_pipeline_status()` to wait for completion.

8. **Stale kubeconfig** — after dev cluster restart or Docker Desktop restart, the kubeconfig may become stale (API server port changes). Refresh it:
   ```bash
   just cluster-down && just cluster-up
   ```

9. **`.sqlx/` stale after Rust code changes** — `cargo sqlx prepare` must be re-run whenever `sqlx::query!` macros change in Rust code, not just when migration SQL changes. The `SQLX_OFFLINE=true` build will fail if the cache is stale:
    ```bash
    just db-prepare   # regenerate .sqlx/ cache
    ```

10. **AppState changes require test helper updates** — when fields are added to `AppState`, both `tests/helpers/mod.rs` and `tests/e2e_helpers/mod.rs` must be updated. Missing fields cause all integration and E2E tests to fail to compile.

11. **Never add FLUSHDB to test helpers** — all Valkey keys are UUID-scoped and never collide between parallel tests. The admin token is created directly in the DB, bypassing the only cross-test key (`rate:login:admin`). FLUSHDB caused flaky failures when one test wiped another's in-flight upload sessions.

12. **Use `admin_token` from `test_state()`, not `admin_login()`** — `test_state()` returns `(AppState, String)` where the second value is a pre-created admin API token. Only call `admin_login()` if you are specifically testing login/session behavior. Using `admin_login()` in all hundreds of parallel tests would exceed the login rate limit (10/300s).

## Cluster Management

```bash
just cluster-up      # create dev cluster + all services + buckets + namespaces
just cluster-down    # destroy dev cluster completely

# Manual cluster recreation (if config changes)
just cluster-down
just cluster-up
```

**What `just cluster-up` provisions** (via `hack/cluster-up.sh`):
- dev cluster with `hack/kind-config.yaml` (port mappings + `/tmp/platform-e2e` mount)
- CNPG-managed Postgres at `localhost:5432` (user: `platform`, password: `dev`, db: `platform_dev`)
- Valkey at `localhost:6379`
- MinIO at `localhost:9000` (S3 API) / `localhost:9001` (console), credentials: `platform`/`devdevdev`
- MinIO buckets: `platform` and `platform-e2e`
- Shared directory: `/tmp/platform-e2e`
- OTel Collector (for observe module)
- `CREATEDB` grant for `platform` DB user (required by `#[sqlx::test]`)

Note: the always-running cluster services (via `just cluster-up`) are used for ad-hoc development and manual testing. The `just test-integration` and `just test-e2e` commands deploy their own ephemeral services in isolated namespaces — they don't use the shared cluster services.

## CI Integration

### Local CI

```bash
just ci              # fmt + lint + deny + test-unit + test-integration + build
just ci-full         # ci + test-e2e (the full verification suite)
```

Both `just ci` and `just ci-full` require a running dev cluster since integration tests deploy ephemeral services inside it. `just test-unit` can run standalone without any infrastructure.

**Always run `just ci-full` before considering work complete.** E2E tests catch real issues that unit and integration tests miss.

### GitHub Actions CI (`.github/workflows/ci.yaml`)

The CI workflow runs all three test tiers plus linting and build:

| Job | What it does | Services |
|---|---|---|
| `fmt` | `cargo fmt --check` | None |
| `lint` | `cargo clippy -- -D warnings` | None |
| `test-unit` | `cargo nextest run --lib` | None |
| `test-integration` | `hack/test-in-cluster.sh --filter '*_integration'` | dev cluster with ephemeral services |
| `test-e2e` | `hack/test-in-cluster.sh --type e2e` | dev cluster with ephemeral services |
| `deny` | `cargo deny check` | None |
| `coverage` | Unit + integration coverage → Codecov | dev cluster with ephemeral services |
| `build` | `cargo build --release` (amd64 + arm64) | None (depends on all test jobs) |

The build job gates on all test tiers — a failing E2E test blocks the release build.

## Coverage

Three-tier coverage reporting using [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) with separate reports for unit, integration, and E2E tests. This makes the testing pyramid visible — if code is only covered by E2E tests, it should probably also have unit tests.

### Prerequisites

```bash
cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
```

### Commands

```bash
just cov-unit         # unit coverage → coverage-unit.lcov (no infra needed)
just cov-integration  # integration coverage → coverage-integration.lcov (ephemeral cluster services)
just cov-e2e          # E2E coverage → coverage-e2e.lcov (ephemeral cluster services)
just cov-total        # ★ combined report: unit + integration + E2E (ephemeral cluster services)
just cov-html         # unit coverage as HTML report → coverage-html/
```

Generated files (`*.lcov`, `coverage-html/`) are gitignored.

All coverage commands except `cov-unit` use `hack/test-in-cluster.sh --coverage` to deploy ephemeral services in isolated cluster namespaces — the same approach used by `just test-integration` and `just test-e2e`. No manual port-forwarding or database setup is needed.

### Combined coverage (the meaningful number)

Separate per-tier coverage is diagnostic. The number that matters is combined: "when all tests run, what % of lines are hit?"

```bash
# Prerequisites: dev cluster running (just cluster-up)
just cov-total
```

Under the hood, `just cov-total` runs `hack/test-in-cluster.sh --type total` which:

1. Creates an ephemeral K8s namespace with fresh Postgres, Valkey, MinIO
2. Cleans previous profiling data (`cargo llvm-cov clean --workspace`)
3. Runs unit tests with coverage instrumentation (`--lib`)
4. Runs integration tests with coverage instrumentation (`--test '*_integration'`)
5. Runs E2E tests with coverage instrumentation (`--test 'e2e_*'`)
6. Generates the combined report (`cargo llvm-cov report`)
7. Cleans up the ephemeral namespace

### Excluded from coverage

- `src/observe/proto.rs` — generated protobuf types
- `src/ui.rs` — rust-embed static file serving
- `src/main.rs` — bootstrap wiring (tested via E2E)
- `tests/`, `ui/`, `mcp/` — non-source code

### CI coverage

The `coverage` job in `.github/workflows/ci.yaml` runs after unit tests pass, generates unit and integration lcov reports (unit tests run directly, integration coverage uses `hack/test-in-cluster.sh` with ephemeral cluster services), and uploads them to Codecov with separate flags (`unit`, `integration`). E2E coverage runs locally via `just cov-total`.

Codecov configuration is in `codecov.yml`:
- **Unit coverage**: gated — target is auto-ratcheting, new code (patch) must have 70% coverage
- **Integration coverage**: informational — tracked but does not block PRs
- **E2E coverage**: informational with carryforward (nightly updates)

### Interpreting results

| Scenario | What it means | Action |
|---|---|---|
| High unit, low integration | Logic tested, wiring not | Add integration tests for key API paths |
| Low unit, high E2E | Logic only tested through slow paths | Extract pure functions, add unit tests |
| Low everywhere | Untested code | Prioritize unit tests for business logic |

### VS Code integration

Install the Coverage Gutters extension (`ryanluker.vscode-coverage-gutters`), run `just cov-unit`, then Cmd+Shift+P → "Coverage Gutters: Display Coverage" to see green/red line indicators inline.
