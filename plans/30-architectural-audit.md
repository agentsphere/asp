# Architectural Audit: Module Hierarchy & Testing Infrastructure

## Context

This is a comprehensive audit of the platform's ~23K LOC single-crate Rust binary. The project implements 11 modules across 14 top-level `mod` declarations. All 25 implementation phases are complete. The goal is to assess architectural health, identify high-impact refactors, and surface Rust best-practice violations.

---

## Phase 1: Module & Visibility Audit

### Overall Structure: Healthy with Fixable Layer Violations

The crate uses `main.rs` + `lib.rs` (mirror), consistent `mod.rs` style throughout, and a flat 2-level module hierarchy (deepest: `agent/claude_code/`). No "God module" per se, but two files are oversized: `pipeline/executor.rs` (1,577 lines) and `observe/alert.rs` (1,188 lines).

### Visibility: Maximally Public, No Encapsulation

| Metric | Count |
|---|---|
| `pub fn/struct/enum/trait/type` | ~320 |
| `pub(crate)` | 4 (all in `api/webhooks.rs`) |
| `pub(super)` | 0 |

**Every module item is fully `pub`.** The lib.rs `pub mod` declarations exist for E2E test access, which is correct, but within the crate there is zero compiler-enforced encapsulation. Architectural boundaries are convention-only.

### Cross-Module Layer Violations (5 found)

These are the most architecturally significant findings. They represent inverted or circular dependency directions:

1. **`store/eventbus.rs` -> `deployer/ops_repo`** (infrastructure -> domain)
   - `eventbus.rs` calls `crate::deployer::ops_repo::{commit_values, revert_last_commit, read_values}`
   - The eventbus contains multi-step deployment workflow logic (commit to git, update DB, wake deployer) that belongs in `deployer/`
   - The store module should be the lowest layer; it shouldn't import domain logic

2. **`notify/webhook.rs` -> `api/webhooks`** (peer module -> api layer)
   - Imports `WEBHOOK_CLIENT`, `WEBHOOK_SEMAPHORE`, `validate_webhook_url` from `api/webhooks.rs`
   - The shared HTTP client/semaphore/SSRF-validation are infrastructure, not API concerns

3. **`{pipeline,deployer,agent,git}` -> `api/webhooks::fire_webhooks`** (domain -> api layer)
   - 11 call sites across 8 files: `pipeline/executor.rs`, `deployer/reconciler.rs`, `agent/service.rs`, `git/hooks.rs`, plus api files
   - `fire_webhooks()` is a utility that reads DB + dispatches HTTP; it shouldn't live in the api handler module

4. **`store/mod.rs` -> `agent::inprocess::InProcessHandle`** (infrastructure -> domain)
   - `AppState` directly imports `InProcessHandle` from the agent module
   - Makes the core state struct depend on agent internals

5. **`validation.rs` -> `agent::provider::BrowserConfig`** (infrastructure -> domain)
   - `check_browser_config()` imports a domain type into the infrastructure validation module

### Stale `#[allow(dead_code)]` Suppressions

All phases are complete, but these remain:
- `src/store/mod.rs:16` — on `AppState` struct
- `src/config.rs:5` — on `Config` struct
- `src/notify/mod.rs` — on all 3 sub-modules
- `src/secrets/mod.rs` — on both sub-modules
- `src/workspace/mod.rs` — on `service` sub-module
- `src/registry/types.rs` — on 5 individual fields
- ~40+ total occurrences that may be masking genuinely unused code

---

## Phase 2: Test Suite Audit

### Test Pyramid: 508 unit + ~271 integration + 40 E2E = ~819 total

**Unit tests (508)**: All inline `mod tests` in source files. Pure logic tests — no DB/network, with one exception. Well-structured with `rstest` parametrize and `proptest` property testing in `validation.rs`.

**Integration tests (271)**: 16 files in `tests/`, using `#[sqlx::test]` for per-test DB isolation. All require Postgres + Valkey.

**E2E tests (40)**: 5 files, all `#[ignore]`, require full Kind cluster.

### Problem: I/O in Unit Tests

`src/deployer/ops_repo.rs` contains 4 `#[tokio::test]` tests that **spawn real `git` processes** and write to temp directories. These run during `just test-unit` (`cargo nextest run --lib`). They should be moved to integration tests or marked `#[ignore]`.

### Problem: Test Helper Duplication

Two separate helper modules with ~300 lines of duplicated code:

| Function | `tests/helpers/mod.rs` | `tests/e2e_helpers/mod.rs` |
|---|---|---|
| `admin_login()` | Yes | Yes (copy) |
| `create_user()` | Yes | Yes (copy) |
| `create_project()` | Yes | Yes (copy) |
| `assign_role()` | Yes | Yes (copy) |
| `get_json()` | Yes | Yes (copy) |
| `post_json()` | Yes | Yes (copy) |
| `patch_json()` | Yes | Yes (copy) |
| `delete_json()` | Yes | Yes (copy) |
| `put_json()` | Yes | **Missing** |
| `get_bytes()` | Missing | Yes |
| Git/K8s helpers | N/A | Yes (unique) |

The only meaningful difference: `test_state()` uses in-memory MinIO; `e2e_state()` uses real S3.

### Problem: Latent Valkey Concurrency Risk

All integration tests share one Valkey instance. `test_state()` calls `FLUSHDB` at setup. If nextest runs tests from multiple files in parallel, one test's `FLUSHDB` can corrupt another test's permission cache. No nextest `[test-groups]` or serialization config exists.

### Problem: Unused `insta` Dev Dependency

`insta` (JSON snapshot testing) is in `[dev-dependencies]` but no `.snap` files exist anywhere in the repo.

### Tight Coupling in Background Tasks

`pipeline/executor.rs`, `deployer/reconciler.rs`, and `agent/service.rs` take `AppState` directly and call K8s APIs inline. No trait abstraction exists for the K8s client, so these are only testable at the E2E level. Unit tests cover the extracted pure helpers (`build_pod_spec`, `extract_exit_code`) but not the orchestration logic.

---

## Phase 3: Build & Dependencies

### Dependency Profile

- 44 direct dependencies, 581 transitive (in `Cargo.lock`)
- 29 proc-macro crates (major compile-time contributors: sqlx, serde, prost, kube, arrow)
- 249 cached sqlx query files in `.sqlx/`

### Actionable Dependency Issues

1. **`arrow = { features = ["json"] }`** — json feature is never used in source code. Remove it.
2. **`tower = { features = ["full"] }`** in production — could enumerate needed features
3. **`insta`** in dev-deps — unused, can be removed
4. **`webauthn-rs`** pulls openssl — tracked in `deny.toml`, awaiting upstream v6.0 fix

### Code Quality Flags

- **Production `.unwrap()`**: 4 instances in `agent/inprocess.rs` on `std::sync::RwLock` (should use `tokio::sync::RwLock` in async context)
- **No `todo!()` or `unimplemented!()`** — clean
- **No `unsafe`** — `forbid` lint working correctly
- **No `extern crate`** — correct for edition 2024
- **`Config` has 27 fields** — borderline but acceptable for a config struct

---

## Top 5 High-Impact Refactors

### 1. Extract `fire_webhooks()` + webhook infrastructure to `src/webhook.rs`

**Impact**: Fixes the biggest layer violation (11 call sites across 8 files importing from `api/webhooks.rs`)

**What to move**: `fire_webhooks()`, `WEBHOOK_CLIENT`, `WEBHOOK_SEMAPHORE`, `validate_webhook_url()`, `dispatch_single()` -> new `src/webhook.rs`

**Files changed**: `src/webhook.rs` (new), `src/api/webhooks.rs`, `src/notify/webhook.rs`, `src/pipeline/executor.rs`, `src/deployer/reconciler.rs`, `src/agent/service.rs`, `src/git/hooks.rs`, `src/api/{issues,merge_requests,deployments,sessions}.rs`

### 2. Move eventbus deployment logic to `deployer/`

**Impact**: Fixes store -> deployer layer violation; makes store a true infrastructure layer

**What to do**: Extract `handle_image_built()` and `handle_rollback_requested()` workflow logic from `store/eventbus.rs` into `deployer/` (e.g., `deployer/event_handler.rs`). The eventbus becomes a dumb router that deserializes events and calls into domain modules.

**Files changed**: `src/store/eventbus.rs`, `src/deployer/mod.rs`, `src/deployer/event_handler.rs` (new)

### 3. Consolidate test helpers into shared `tests/common/mod.rs`

**Impact**: Eliminates ~300 lines of duplicated test code, reduces maintenance burden

**What to do**: Create `tests/common/mod.rs` with shared HTTP helpers (`get_json`, `post_json`, etc.) and auth helpers (`admin_login`, `create_user`, etc.). Both `helpers/mod.rs` and `e2e_helpers/mod.rs` delegate to `common/` for shared code. State setup remains separate (different MinIO backends).

**Files changed**: `tests/common/mod.rs` (new), `tests/helpers/mod.rs`, `tests/e2e_helpers/mod.rs`

### 4. Add `pub(crate)` to internal items + remove stale `#[allow(dead_code)]`

**Impact**: Compiler-enforced module boundaries; surfaces genuinely unused code

**What to do**:
- Remove all `#[allow(dead_code)]` from `AppState`, `Config`, `notify/mod.rs`, `secrets/mod.rs`, `workspace/mod.rs`, `registry/types.rs`
- Fix any resulting compiler errors (genuinely dead code)
- Add `pub(crate)` to items that are internal to their module (webhook statics, internal helpers)

### 5. Add nextest `[test-groups]` for Valkey isolation

**Impact**: Eliminates latent test flakiness from concurrent `FLUSHDB`

**What to do**: Add to `.config/nextest.toml`:
```toml
[test-groups]
valkey = { max-threads = 1 }

[[profile.default.overrides]]
filter = "not test(/^e2e_/)"
test-group = "valkey"
```

Or better: prefix Valkey keys with a per-test random namespace instead of `FLUSHDB`.

---

## Minor Fixes (Low Effort)

- Remove `arrow` `json` feature from Cargo.toml
- Remove `insta` from dev-dependencies
- Move 4 git-spawning tests from `src/deployer/ops_repo.rs` to `tests/`
- Replace `std::sync::RwLock` with `tokio::sync::RwLock` for `inprocess_sessions` in AppState
- Move `slug()` / `slugify_branch()` from `pipeline/mod.rs` to a shared `src/util.rs` (used by both pipeline and deployer)
- Move `check_browser_config()` from `validation.rs` to `agent/provider.rs`

---

## Files Violating Rust Best Practices

| File | Issue |
|---|---|
| `src/store/eventbus.rs` | Infrastructure module imports domain logic (`deployer::ops_repo`) |
| `src/notify/webhook.rs` | Imports shared infra from `api/webhooks.rs` (inverted dependency) |
| `src/store/mod.rs` | `AppState` imports `agent::inprocess::InProcessHandle` (core struct depends on domain) |
| `src/validation.rs` | Imports `agent::provider::BrowserConfig` (infra imports domain type) |
| `src/notify/mod.rs` | Blanket `#[allow(dead_code)]` on all sub-modules (stale suppression) |
| `src/secrets/mod.rs` | Same — blanket dead_code suppression |
| `src/agent/inprocess.rs` | Uses `std::sync::RwLock` in async context with `.unwrap()` |
| `src/deployer/ops_repo.rs` | Unit test module spawns `git` processes (should be integration) |

---

## Verification

After implementing the refactors:
1. `just ci` — fmt, lint, deny, test-unit, build all pass
2. `just test-integration` — integration tests pass
3. `just test-e2e` — E2E tests pass (if cluster available)
4. Verify no new `#[allow(dead_code)]` was needed after removing stale ones
5. Confirm `pub(crate)` additions don't break E2E tests (lib.rs re-exports remain `pub`)
