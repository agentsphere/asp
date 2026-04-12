# Lessons Learned & Monolith-First Requirements

## Context

We attempted a two-phase crate restructure of the platform (103K LOC monolith
with 2 crates: proxy, proxy-init):

1. **Service decoupling** — extracted 4 standalone binaries (executor, deployer,
   ingest, alert-eval) + 11 shared library crates, communicating via Valkey queues
2. **True compile boundaries** — tried to move business logic into domain crates
   so changes in one domain don't recompile others

Phase 1 succeeded at runtime separation but copy-pasted ~20K LOC into binary
crates. Phase 2 consolidated ~12.5K LOC of that duplication but hit a hard
ceiling: domain functions use `AppState` + `sqlx::query!()`, which can't move
to external crates without losing type safety or rewriting every function
signature.

**Current state**: src/ = 84K LOC, crates/ = 45K LOC (16 crates, up from 2).
The 4 binary crates still carry adapted copies of domain code with their own
state types and dynamic (not compile-time) SQL queries.

**Old monolith** (cloned to `/Users/steven/newKid/business/agentsphere/repos/oldAsp`):
103K LOC, 2 crates, single binary, all background tasks in-process via
`Arc<tokio::sync::Notify>`.

Rather than continuing to force separation, this document catalogs every gotcha
we hit and defines what the monolith needs to change *internally* before any
future extraction attempt.

---

## What Worked

1. **Valkey queues replacing Arc<Notify>** — `platform_queue::publish_trigger()`
   / `subscribe_trigger()` cleanly replaced `pipeline_notify.notify_one()` and
   `deploy_notify.notify_one()`. Both the main binary and standalone binaries
   now use the same Valkey trigger channels. AppState dropped 3 fields
   (`pipeline_notify`, `deploy_notify`, `secret_requests`).

2. **Pure-type crates** — `platform-types`, `platform-observe-types`,
   `platform-pipeline-defs`, `platform-config`, `platform-validation` extracted
   trivially because they have zero I/O and no AppState dependency.

3. **Re-export pattern** — `pub use platform_crate::*;` in src/ modules preserved
   all 76 test files' `platform::*` imports with zero test rewrites.

4. **Infrastructure crates** — `platform-k8s`, `platform-webhook`,
   `platform-secrets-engine`, `platform-queue` work well as shared libraries
   consumed by both src/ and standalone binaries.

## What Failed

1. **AppState is a god object** — 13 fields, passed to 64 functions. Domain
   functions that only need `(&PgPool, &Config)` still receive `&AppState`
   because that's what axum handlers have.

2. **`sqlx::query!()` locks code to the compilation unit** — Moving queries to
   external crates means either (a) `cargo sqlx prepare` per crate with a live
   DB, or (b) converting to `sqlx::query()` (losing compile-time checking).
   Binary crates chose (b): all 76 queries across 4 binary crates are dynamic.
   Schema changes now require updating queries in 2 places with no compiler help.

3. **No domain service layer** — API handlers query the DB directly AND call
   domain functions. Extracting a domain requires also extracting its SQL
   queries from scattered handler files.

4. **Binary crates needed full code copies** — Because domain functions take
   `&AppState`, standalone binaries couldn't reuse them. Each binary defined
   its own state type (ExecutorState, DeployerState, IngestState, AlertEvalState)
   and adapted copies of every function.

5. **Type duplication** — `Permission` enum is defined in both `platform-types`
   and `platform-auth`. Same for `UserType`. Divergence risk with no compiler
   warning.

6. **Dead code suppressions everywhere** — 12+ `#[allow(dead_code)]` in crates
   indicate incomplete wiring. Library code was extracted but not all entry
   points connected.

7. **Zero tests in binary crate test directories** — All meaningful tests stayed
   in src/'s `tests/` directory. Binary crate logic is only validated via
   integration tests against the main binary — not the actual binary that runs
   in production.

8. **Config is all-or-nothing** — `platform-config::Config` has ~30 fields. A
   binary that doesn't need `minio_endpoint` still fails if the env var is
   missing, because Config parsing is monolithic.

---

## Complete Gotcha Catalog

Every specific issue we hit, organized by category:

### G1. AppState Coupling (blocked all code sharing)

**What happened**: Every background task function (`run()`, `run_reaper()`,
`evaluate_alerts_loop()`) takes `AppState`. The old monolith passed `state.clone()`
to each spawned task. When we extracted binaries, we couldn't call these functions
because they require an `AppState` with 13 fields — most of which the binary
doesn't have.

**Concrete example** (old monolith `src/main.rs`):
```rust
tracker.spawn(pipeline::executor::run(state.clone(), token.clone()));
tracker.spawn(deployer::reconciler::run(state.clone(), token.clone()));
```

The executor only uses `pool`, `valkey`, `kube`, `config`, `minio`,
`task_registry`, `webhook_semaphore` (7 of 13 fields). But the function
signature requires all 13.

**Binary crate workaround**: Define `ExecutorState` with just 7 fields, then
copy-paste `executor::run()` and change `state: AppState` → `state: ExecutorState`.
This created 2,985 LOC of duplicated executor code.

### G2. Compile-Time SQL → Dynamic SQL Downgrade

**What happened**: `sqlx::query!()` requires either a live DB or an `.sqlx/`
offline cache at compile time. The `.sqlx/` cache is generated per-crate by
`cargo sqlx prepare`. Binary crates would need their own `.sqlx/` caches,
requiring a live DB during CI. Rather than add that complexity, all binary crates
converted to `sqlx::query()` (dynamic).

**Impact**: 76 queries across 4 binary crates lost compile-time checking. A
schema change (column rename, type change) breaks at runtime, not compile time.
The main binary's 200+ `sqlx::query!()` calls still have compile-time checking,
so the same schema change gets caught there — but the binary crate version of
the same query silently breaks.

### G3. Re-Export Seams Hide Breaking Changes

**What happened**: 18+ files in src/ became thin re-exports:
```rust
// src/deployer/applier.rs (6 lines)
pub use platform_deployer::applier::*;
```

Tests import `platform::deployer::applier::*` and still work. But if the
crate's API changes (function renamed, parameter added), the re-export silently
passes it through. No compilation error in the main binary unless a caller
actually uses the changed function. Uncalled re-exports are invisible.

### G4. Dual Error Type Bridges

**What happened**: Binary crates define their own error types (e.g.,
`DeployerError`). The main binary needs `From<DeployerError> for ApiError`.
If the crate adds a new error variant, the `From` impl in src/error.rs doesn't
cover it — but this only fails if a code path actually triggers that variant.
Non-exhaustive match in error bridges creates latent bugs.

### G5. Type Duplication (Permission, UserType)

**What happened**: `platform-types` was created as a leaf crate (no I/O deps).
`platform-auth` also needed `Permission` and `UserType` but couldn't depend on
`platform-types` at the time (or vice versa). Both crates now define the same
enums independently.

**Files**:
- `crates/platform-types/src/permission.rs`
- `crates/platform-auth/src/rbac/types.rs`
- `crates/platform-types/src/user_type.rs`
- `crates/platform-auth/src/user_type.rs`

### G6. Config Monolith

**What happened**: `platform-config::Config` parses all env vars eagerly. The
executor binary doesn't need `WEBAUTHN_RP_ID` or `PLATFORM_SSH_LISTEN`, but
Config::load() fails if they're missing (unless defaulted). Binary deployments
require setting 30+ env vars even if unused.

### G7. Dead Code in Extracted Crates

**What happened**: When deployer code was extracted to `platform-deployer`,
entire modules were copied (applier, ops_repo, renderer, preview, gateway).
Not all functions are called from the binary's `main()`. The crate-wide
`#![allow(dead_code)]` suppresses warnings, hiding genuinely unused code.

**Files with suppressions**:
- `crates/platform-deployer/src/lib.rs` — crate-wide
- `crates/platform-executor/src/lib.rs` — crate-wide
- `crates/platform-ingest/src/state.rs`
- `crates/platform-ingest/src/auth.rs`
- `crates/platform-ingest/src/proto.rs`
- `crates/platform-deployer/src/ops_repo.rs`
- `crates/platform-deployer/src/preview.rs`
- Plus 4+ more files

### G8. Binary Crate Tests Don't Exist

**What happened**: All 76 test files are in the main binary's `tests/` dir,
importing `platform::*`. Binary crates have `[dev-dependencies]` in Cargo.toml
but no actual test files in their `tests/` directories. The ~30 inline
`#[cfg(test)]` tests in binary crates are mostly trivial unit tests.

**Result**: Production binaries (executor, deployer, ingest, alert-eval) are
validated by tests that run against the main binary's code, not the binary
crate's adapted copies. A bug introduced by the `AppState → ExecutorState`
adaptation would not be caught.

### G9. Webhook Dispatch Duplication

**What happened**: `platform-webhook` crate exists for shared webhook logic,
but the executor binary has its own `crate::webhook` module. Two implementations
of webhook dispatch exist with different retry/templating logic.

### G10. Vestigial Ingest Code in Main Binary

**What happened**: `src/observe/ingest.rs` still exports `IngestChannels`,
`create_channels`, `BUFFER_CAPACITY`. These are only used by the `platform-ingest`
binary, not the main binary. The main binary no longer runs ingest — it's a
separate service. But the code stays, creating confusion about responsibility.

### G11. kind_to_plural Manual Mapping

**What happened**: `platform-k8s::kind_to_plural()` has a hardcoded match
statement for K8s resource type → plural name. The fallback appends "s", which
is wrong for irregular plurals (`NetworkPolicy` → `networkpolicies`, not
`networkpolicys`). Every new K8s resource type requires a manual addition.

### G12. Webhook Semaphore is a Global Singleton

**What happened**: `platform-webhook` uses a static `OnceLock<Semaphore>` with
50 slots. In a multi-service deployment, each process gets its own semaphore
(that's fine). But within a single process, the semaphore is shared across all
callers. If the main binary and an in-process webhook task both dispatch, they
compete for the same 50 slots.

### G13. fred Pool PubsubInterface Trap

**What happened**: `fred::clients::Pool` does NOT implement `PubsubInterface`.
Must call `pool.next()` to get a `Client`. For dedicated subscribers, need
`pool.next().clone_new()` to avoid interfering with pooled connections.
`platform-queue` wraps this but the gotcha resurfaces whenever someone writes
direct fred code.

### G14. sqlx Migration `-- no-transaction` Directive

**What happened**: For `CREATE INDEX CONCURRENTLY`, the migration's first line
must be exactly `-- no-transaction`. NOT `-- sqlx-disable-transaction`. sqlx
silently ignores wrong directives, wraps the DDL in a transaction, and Postgres
errors with "cannot run CREATE INDEX CONCURRENTLY inside a transaction".

---

## Requirements for Future Clean Separation

These changes should happen *inside the monolith* before any future extraction.
Each is independently valuable — they improve the codebase whether or not we
ever extract crates.

### R1. Domain Functions Take Explicit Parameters, Not AppState

**Problem**: 64 functions take `&AppState` but most use 1-3 fields. This is
the single root cause of G1 (AppState coupling) and G8 (binary copies).

**Change**: Domain functions accept only what they need:
```rust
// Before
pub async fn collect_garbage(state: &AppState) { /* uses state.pool, state.minio */ }

// After
pub async fn collect_garbage(pool: &PgPool, minio: &Operator) { ... }
```

For functions needing 4+ params, define a params struct in the module:
```rust
pub struct ExecutorParams<'a> {
    pub pool: &'a PgPool,
    pub kube: &'a kube::Client,
    pub config: &'a Config,
    pub minio: &'a Operator,
    pub valkey: &'a fred::clients::Pool,
    pub webhook_semaphore: &'a Semaphore,
    pub task_registry: &'a TaskRegistry,
}
```

API handlers destructure at the call site:
```rust
async fn handler(State(state): State<AppState>, ...) {
    collect_garbage(&state.pool, &state.minio).await?;
}
```

**Scope**: ~64 functions. Module-by-module, starting with least-coupled:
1. `notify/dispatch.rs` — 7 handlers, each uses pool + 0-1 other fields
2. `registry/gc.rs` — pool + minio
3. `deployer/analysis.rs` — pool + task_registry
4. `observe/alert.rs` — pool + valkey + audit_tx
5. `agent/service.rs` — mixed, some need 5 fields → params struct
6. `pipeline/executor.rs` — 7 fields → `ExecutorParams`
7. `deployer/reconciler.rs` — 6 fields → `ReconcilerParams`

**Priority**: HIGH — this is the #1 enabler. Once done, binary crates can call
src/ functions directly without copying them.

**Addresses**: G1, G8, G9

### R2. Consolidate SQL Queries into Domain Modules

**Problem**: 667 `sqlx::query!()` calls spread across 74 files. API handlers
do direct DB access alongside domain function calls. No clear data boundary.

**Change**: Each domain module gets a `queries.rs` that owns all SQL for that
domain. Handlers call domain functions, not raw SQL:
```rust
// Before (in api/projects.rs)
let project = sqlx::query!("SELECT * FROM projects WHERE id = $1", id)
    .fetch_optional(&state.pool).await?;

// After
let project = workspace::get_project(&state.pool, id).await?;
```

**Scope**: Start with the 6 most query-heavy files:
- pipeline/executor.rs (49 queries)
- api/merge_requests.rs (43 queries)
- deployer/reconciler.rs (28 queries)
- api/admin.rs (26 queries)
- api/deployments.rs (25 queries)
- agent/service.rs (24 queries)

Queries stay as `sqlx::query!()` — they just move to domain modules. Compile-time
checking is preserved. When extracting to a crate later, that crate runs
`cargo sqlx prepare` once to generate its `.sqlx/` cache.

**Priority**: MEDIUM — large refactor but makes future extraction trivial.

**Addresses**: G2, G3

### R3. Per-Domain Error Types (Complete the Pattern)

**Problem**: Many domain functions return `Result<T, ApiError>` directly,
coupling domain logic to the HTTP layer.

**Already done**: pipeline, deployer, auth, agent, observe (partial), ingest
**Still needed**: git, registry, notify, secrets, workspace, mesh

Add `From<DomainError> for ApiError` impls in src/error.rs. ~5-10 lines each.

**Priority**: LOW — most important domains already have this.

**Addresses**: G4

### R4. Deduplicate Permission and UserType Enums

**Problem**: Both enums defined in `platform-types` AND `platform-auth`.

**Change**: Keep definitions only in `platform-types`. Have `platform-auth`
depend on `platform-types` and re-export. If there's a circular dep issue,
the auth-specific variants should move to `platform-types` (it's a leaf crate
with no I/O, so adding enum variants is fine).

**Priority**: HIGH — divergence is a ticking time bomb.

**Addresses**: G5

### R5. Modular Config Loading

**Problem**: `Config::load()` parses all 30+ env vars eagerly. Binary crates
that don't need all fields fail if vars are missing.

**Change**: Either (a) make fields that only the main binary needs `Option<T>`
with sensible defaults, or (b) split Config into sub-structs:
```rust
pub struct Config {
    pub core: CoreConfig,      // database_url, valkey_url, listen — always required
    pub storage: StorageConfig, // minio_* — required by executor, deployer, ingest
    pub auth: AuthConfig,       // webauthn_*, secure_cookies — only main binary
    pub mesh: MeshConfig,       // mesh_* — only main binary
    pub git: GitConfig,         // git_repos_path, ssh_* — only main binary
}
```

Binary crates construct only the sub-configs they need.

**Priority**: MEDIUM — reduces deployment friction for standalone binaries.

**Addresses**: G6

### R6. Move ListResponse to platform-types

**Problem**: `observe/alert.rs` imports `api::helpers::ListResponse` — the only
domain module importing from api/. Back-reference.

**Change**: Move `ListResponse<T>` to `platform-types` (generic pagination
wrapper, zero framework deps).

**Priority**: LOW — single-file change.

### R7. Remove Dead Code Suppressions

**Problem**: 12+ `#[allow(dead_code)]` in crates hide unused code.

**Change**: Remove all crate-wide `#![allow(dead_code)]`. Fix resulting warnings
by either wiring unused functions or deleting them.

**Priority**: LOW — do before any future extraction attempt.

**Addresses**: G7

### R8. Split Eventbus Handlers to Domain Modules

**Problem**: `store/eventbus.rs` is 2,120 lines with 5 handler functions + 18
queries. Application glue that knows about all domains.

**Change**: Each domain module exports event handlers. Eventbus becomes a thin
dispatcher:
```rust
match event {
    PlatformEvent::ImageBuilt { .. } => registry::on_image_built(pool, ...).await,
    PlatformEvent::ReleasePromoted { .. } => deployer::on_release_promoted(pool, ...).await,
}
```

**Priority**: LOW — works fine as-is, do when touching eventbus.

### R9. Clean Up Vestigial Ingest Code

**Problem**: Main binary's `src/observe/ingest.rs` exports types only used by
the ingest binary.

**Change**: Remove the non-`#[cfg(test)]` re-exports of `IngestChannels`,
`create_channels`, `BUFFER_CAPACITY` from `src/observe/ingest.rs`. If tests
need them, gate behind `#[cfg(test)]`.

**Priority**: LOW — cosmetic cleanup.

**Addresses**: G10

---

## Recommended Execution Order

```
1. R1 — Explicit params, not AppState (HIGH, ~2 days)
   The #1 enabler. Start with notify, registry/gc, deployer/analysis.
   Then pipeline/executor, deployer/reconciler, agent/service.

2. R4 — Deduplicate Permission/UserType (HIGH, ~1 hour)
   Quick win, removes divergence risk.

3. R5 — Modular Config (MEDIUM, ~1 day)
   Reduces deployment friction for standalone binaries.

4. R2 — Query consolidation (MEDIUM, ~3 days)
   Module by module, incrementally. Can overlap with normal development.

5. R3, R6-R9 — Error types, ListResponse, dead code, eventbus, vestigial (LOW)
   Opportunistic — do when touching those files.
```

**After R1 + R4 + R5 are done**, standalone binary crates can be rewritten as
thin main.rs files (~100-200 LOC each) that call src/ domain functions directly:

```rust
// crates/platform-executor/src/main.rs
use platform::pipeline::executor::{self, ExecutorParams};

#[tokio::main]
async fn main() {
    let config = platform_config::Config::load();
    let pool = PgPool::connect(&config.core.database_url).await?;
    // ... build params from config ...
    executor::run(ExecutorParams { pool: &pool, ... }, cancel).await;
}
```

This eliminates ~20K LOC of duplicated code, restores compile-time SQL checking
for binary crates (they compile against the `platform` lib which has `.sqlx/`),
and makes tests cover the actual production code path.

## What NOT to Do

- **Don't extract more domain crates** — pure-type crates work. Crates with
  DB queries create the same problems (G2, G8).
- **Don't convert `sqlx::query!()` to `sqlx::query()`** — losing compile-time
  checking is not worth crate boundary flexibility.
- **Don't rewrite test helpers** — the re-export pattern (`platform::*`) works.
- **Don't add a "platform-error" crate** — ApiError is HTTP-specific, stays in src/.
- **Don't try to slim AppState itself** — it's appropriate for the coordinator.
  The fix is making domain functions not depend on it (R1).
- **Don't add per-crate `.sqlx/` caches** — maintenance burden outweighs benefit.
  Better to have binaries depend on the `platform` lib crate directly.

## Verification

After completing R1 + R4 + rewriting binary crates:
```bash
SQLX_OFFLINE=true cargo check --workspace   # all crates compile
just test-unit                               # unit tests pass
just test-integration                        # integration tests pass
just ci-full                                 # full CI green
```

**Expected outcome**:
- Binary crate LOC: each <200 lines (down from 1.2K-12.3K)
- Total crate LOC: ~25K (down from ~45K)
- No code duplication between src/ and crates/
- All SQL queries compile-time checked (no more dynamic queries in binaries)
- Zero `#[allow(dead_code)]` in crates
