# Plan 29: Event-Driven Deploy via Ops Repo as Source of Truth

## Context

Currently the pipeline executor directly writes `image_ref` to the `deployments` table after a successful build, and the deployer reconciler polls every 10s to pick up changes. The ops repo is just a template store — the actual image ref lives only in the DB. Rollback re-renders the template with a previous image ref but never touches the ops repo.

The goal: make the **ops repo the source of truth** for what's deployed. Pipeline completion commits a values file to the ops repo, the deployer reads from it, and rollback reverts the ops repo commit. A Valkey pub/sub event bus replaces polling for deploy triggers.

Also: ops repos become **local bare repos** (like project repos), not remote URLs.

---

## 1. Migrate Ops Repos to Local Bare Repos

**Migration**: `ALTER TABLE ops_repos` — replace `repo_url` with `repo_path TEXT`, drop `sync_interval_s` (no remote syncing needed).

**File: `src/deployer/ops_repo.rs`** — rewrite:
- Remove `clone_repo()`, `pull_repo()`, SSRF validation
- Remove Valkey sync caching (no remote to cache)
- `init_ops_repo(repos_dir, name, branch)` — calls `git init --bare`, returns path. Reuse pattern from `src/git/repo.rs:init_bare_repo()`
- `get_head_sha(repo_path)` — keep as-is (already works on local repos)
- `resolve_manifest_path()` — keep as-is
- `sync_repo()` → simplify to just `get_head_sha()` (no fetch needed for local repos)

**File: `src/api/deployments.rs`** — update ops repo CRUD:
- `create_ops_repo()`: call `init_ops_repo()` to create bare repo on disk, store `repo_path` in DB
- Remove `repo_url` field from request/response types, replace with `repo_path`
- Remove `force_sync_ops_repo()` endpoint (no remote to sync from)
- Remove SSRF validation on ops repo URL

**New migration file**: rename column + drop `sync_interval_s`

---

## 2. Values File Commit via Git Worktree

**File: `src/deployer/ops_repo.rs`** — add:

```
commit_values(repo_path, branch, environment, values: serde_json::Value) -> Result<String>
```

Flow:
1. `git worktree add _values_worktree_{uuid} {branch}` (pattern from `src/api/merge_requests.rs:882-938`)
2. Write `values/{environment}.yaml` with YAML content (image_ref, project_name, etc.)
3. `git add values/{environment}.yaml`
4. `git commit -m "deploy({environment}): update image to {image_ref}"`
5. `git worktree remove --force`
6. Return the new commit SHA

```
revert_last_values_commit(repo_path, branch) -> Result<String>
```

Flow:
1. `git worktree add _revert_worktree_{uuid} {branch}`
2. `git revert HEAD --no-edit` (reverts the last values commit)
3. `git worktree remove --force`
4. Return the new commit SHA

```
read_values(repo_path, branch, environment) -> Result<serde_json::Value>
```

Flow:
1. `git show {branch}:values/{environment}.yaml` (read file from bare repo without worktree)
2. Parse YAML to `serde_json::Value`

---

## 3. Valkey Event Bus Module

**New file: `src/store/eventbus.rs`**

Typed events:
```rust
enum PlatformEvent {
    ImageBuilt { project_id, environment, image_ref, pipeline_id, triggered_by },
    OpsRepoUpdated { project_id, ops_repo_id, environment, commit_sha },
    DeployRequested { project_id, environment },
}
```

Publisher:
```rust
pub async fn publish(valkey: &fred::clients::Pool, event: &PlatformEvent) -> Result<()>
// Serializes to JSON, publishes to channel "platform:events"
```

Subscriber loop (background task):
```rust
pub async fn run(state: AppState, shutdown: watch::Receiver<()>)
// Uses fred SubscriberClient to listen on "platform:events"
// Deserializes events and dispatches to handlers
```

Handler dispatch:
- `ImageBuilt` → calls `ops_repo::commit_values()` to write new image ref to the ops repo, then publishes `OpsRepoUpdated`
- `OpsRepoUpdated` → updates `deployments` table (image_ref + current_status = 'pending'), notifies deployer via `deploy_notify`
- `DeployRequested` → same as OpsRepoUpdated (for manual API triggers)

**File: `src/store/mod.rs`** — add `pub mod eventbus;`, add `deploy_notify: Arc<Notify>` to AppState

**File: `src/store/valkey.rs`** — remove `#[allow(dead_code)]` from `publish()`, add `subscribe()`:
```rust
pub async fn subscribe(pool: &fred::clients::Pool, channel: &str) -> Result<impl Stream<Item = String>>
```

---

## 4. Wire Pipeline → Event Bus

**File: `src/pipeline/executor.rs`**

Replace `detect_and_write_deployment()` body:
- Keep the kaniko detection + image_ref construction logic
- Instead of writing directly to `deployments`/`preview_deployments` tables, publish `ImageBuilt` event:
  ```rust
  eventbus::publish(&state.valkey, &PlatformEvent::ImageBuilt {
      project_id, environment, image_ref, pipeline_id, triggered_by
  }).await;
  ```
- For main/master → `environment = "production"`
- For other branches → `environment = "preview"`

Preview deployments still write directly to `preview_deployments` table (they don't use ops repos).

---

## 5. Wire Event Bus → Deployer

**File: `src/deployer/reconciler.rs`**

Change the reconcile loop to also wake on `deploy_notify`:
```rust
tokio::select! {
    _ = shutdown.changed() => break,
    _ = tokio::time::sleep(Duration::from_secs(10)) => { reconcile(&state).await; }
    () = state.deploy_notify.notified() => { reconcile(&state).await; }
}
```

Modify `render_manifests()`:
- After syncing the ops repo (now just getting HEAD SHA), also `read_values()` for the environment
- Merge the values from the ops repo's values file into the template rendering context
- The `image_ref` now comes from the ops repo values file, not the `deployments.image_ref` column

Modify `handle_rollback()`:
- Call `ops_repo::revert_last_values_commit()` to git-revert the ops repo
- Read the reverted values to get the old image_ref
- Update `deployments.image_ref` in DB to match (for history/API consistency)
- Then render + apply as before

---

## 6. Wire Deployment API → Event Bus

**File: `src/api/deployments.rs`**

`update_deployment()`:
- Instead of directly setting `current_status = 'pending'`, publish `DeployRequested` event
- The event handler commits the new image_ref to ops repo and wakes the deployer

`rollback_deployment()`:
- Publish a rollback event that triggers `revert_last_values_commit()`

---

## 7. Main.rs Wiring

**File: `src/main.rs`**

Add to background tasks:
```rust
let eventbus_shutdown_rx = shutdown_tx.subscribe();
tokio::spawn(store::eventbus::run(state.clone(), eventbus_shutdown_rx));
```

---

## Files Modified

| File | Change |
|------|--------|
| `src/store/mod.rs` | Add `pub mod eventbus`, add `deploy_notify` to AppState |
| `src/store/valkey.rs` | Remove dead_code on `publish()`, add `subscribe()` |
| `src/store/eventbus.rs` | **New** — event types, publisher, subscriber loop, handlers |
| `src/deployer/ops_repo.rs` | Rewrite for local repos; add `init_ops_repo`, `commit_values`, `revert_last_values_commit`, `read_values` |
| `src/deployer/reconciler.rs` | Add `deploy_notify` wake-up; read values from ops repo; fix rollback to revert ops repo |
| `src/pipeline/executor.rs` | Replace `detect_and_write_deployment` with event publish |
| `src/api/deployments.rs` | Update ops repo CRUD for local repos; wire update/rollback through event bus |
| `src/main.rs` | Spawn eventbus background task, construct `deploy_notify` |
| `migrations/` | New migration: ops_repos schema change (repo_url → repo_path, drop sync_interval_s) |

## Verification

1. `just test-unit` — all existing unit tests pass (ops_repo tests need updating for local repo model)
2. `just lint` — no clippy warnings
3. `just test-integration` — deployment integration tests pass with new flow
4. Manual E2E: create project → push code with kaniko step in `.platform.yaml` → pipeline runs → event published → values committed to ops repo → deployer picks up → K8s deployment created
5. Rollback: trigger rollback via API → ops repo commit reverted → deployer re-deploys old image
