# Plan: Fix Audit Findings A1-A25 (excluding A3, A5, A11, A12)

## Context

The 2026-03-26 codebase audit found 9 CRITICAL and 25 HIGH findings. The user accepted A3 (pod privilege escalation — by design), A5 (setup commands — by design), A11 (resolve_image validation — accepted), A12 (global CLI session identity — accepted for now). A6 is split: remove password from log but keep setup token. This plan addresses the remaining 21 findings in 4 batches.

## Batch 1: Auth/RBAC Bypass Fixes (A1, A2, A7, A8, A10)

### A1 (CRITICAL): Owner bypasses RBAC — `src/api/projects.rs`

**update_project (~line 659):** Replace owner-bypass block with:
```rust
require_project_write(&state, &auth, id).await?;
```
Remove the `if project_owner != auth.user_id` conditional. The `require_project_write` helper handles workspace-derived permissions, so owners still pass if they have legitimate access.

**delete_project (~line 758):** Replace owner-bypass with:
```rust
require_admin(&state, &auth).await?;
```
Only admins should delete projects. Remove the owner_id == auth.user_id shortcut.

### A2 (CRITICAL): MR author bypasses RBAC — `src/api/merge_requests.rs`

**update_mr (~line 417):** Add before the author_id query:
```rust
// A2: Even the author needs current project-write permission
require_project_write(&state, &auth, id).await?;
```
Keep the author check below for the "non-author needs admin" gate.

### A7 (CRITICAL): Alert token scope bypass — `src/observe/alert.rs`

Change `require_alert_manage` (~line 175) and `require_observe_read` (~line 191):
- `has_permission()` → `has_permission_scoped()` with `auth.token_scopes.as_deref()` as 6th arg

### A8 (HIGH): Token scope bypass in Git/LFS/Registry

1. Add `token_scopes: Option<Vec<String>>` to `GitUser` struct in `src/git/smart_http.rs`
2. Populate from API token lookup in `authenticate_basic` (add `scopes` to SELECT)
3. Set `None` for password auth and SSH auth
4. Change `has_permission()` → `has_permission_scoped()` in:
   - `src/git/smart_http.rs` `check_access_for_user` (~line 632)
   - `src/git/lfs.rs` (~line 116)
5. Add `token_scopes: Option<Vec<String>>` to `RegistryUser` in `src/registry/auth.rs`
6. Populate from token lookup, change `has_permission()` → `has_permission_scoped()` in `src/registry/mod.rs` (~line 170)
7. Set `None` for SSH key auth in `src/git/ssh_server.rs`

### A10 (HIGH): list_projects ignores token boundary — `src/api/projects.rs`

Add WHERE clauses to both count and main queries in `list_projects` (~line 498):
```sql
AND ($N::uuid IS NULL OR id = $N)           -- boundary_project_id
AND ($M::uuid IS NULL OR workspace_id = $M) -- boundary_workspace_id
```
Bind `auth.boundary_project_id` and `auth.boundary_workspace_id`.

---

## Batch 2: Input Sanitization & Injection (A9, A16, A17, A18, A21, A24)

### A9 (HIGH): Webhook URL logged — `src/api/webhooks.rs`

Replace `url` with `webhook_id = %webhook_id` in all 4 tracing calls in `dispatch_single` (~lines 507, 514, 535, 538). Add `webhook_id: Uuid` parameter, pass from caller.

### A16 (HIGH): YAML injection in reconciler — `src/deployer/reconciler.rs`

Add `validation::check_container_image(&release.image_ref)?` before the `format!` in `generate_basic_manifest` (~line 1260). Map validation error to deployer error.

### A17 (HIGH): Shell interpolation in pipeline — `src/pipeline/executor.rs`

Pass `repo_clone_url` as env var `GIT_CLONE_URL` instead of interpolating into shell string (~line 1518). Reference `"$GIT_CLONE_URL"` in the script. Same fix in `src/agent/claude_code/pod.rs` git-clone init container (~line 780).

### A18 (HIGH): Git ref not validated in trigger — `src/pipeline/trigger.rs`

Add at top of `on_push`, `on_mr`, `on_tag`:
```rust
validation::check_branch_name(&params.branch)
    .map_err(|e| PipelineError::InvalidDefinition(e.to_string()))?;
```

### A21 (HIGH): ILIKE injection in observe search — `src/observe/query.rs`

Escape metacharacters before wrapping (~line 349):
```rust
let search_pattern = params.q.as_deref().map(|s| {
    let escaped = s.replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
});
```

### A24 (HIGH): Broken multi-wildcard glob — `src/validation.rs`

Replace the fallback at ~line 314 with iterative multi-segment matching:
- Split pattern by `*` into segments
- Verify first segment is prefix, last is suffix
- Walk middle segments in order using `find()`
- Add unit tests for `feature/*/fix/*`, `*middle*`, etc.

---

## Batch 3: Memory Bounds & DoS (A13, A14, A15, A19, A20)

### A13 (HIGH): Pub/sub message size — `src/agent/pubsub_bridge.rs`

After `serde_json::to_string` in `publish_event`, check `json.len() > 1_048_576` (1 MB). Return error if exceeded.

### A14 (HIGH): Registry blob reassembly — `src/registry/blobs.rs`

Add `MAX_BLOB_SIZE: u64 = 5 * 1024 * 1024 * 1024` (5 GB). In `complete_upload`, check `session.offset + body.len()` before reading parts. Reject if exceeded.

### A15 (HIGH): Git browser blob — `src/git/browser.rs`

Add `MAX_BLOB_SIZE: usize = 50 * 1024 * 1024` (50 MB). After `git show` output, check `stdout.len()` before conversion. Return 400 if exceeded.

### A19 (HIGH): Observe query timeout — `src/observe/query.rs`

Wrap each query handler's DB call with `tokio::time::timeout(Duration::from_secs(10), ...)`. Map timeout to `ApiError::BadRequest("query timed out")`.

Apply to: `search_logs_inner`, `search_traces_inner`, `get_trace`, `search_metrics_inner`.

### A20 (HIGH): Unbounded trace spans — `src/observe/query.rs`

Add `LIMIT 10000` to the spans query in `get_trace` (~line 537).

---

## Batch 4: Misc Hardening (A4, A6, A22, A23, A25)

### A4 (CRITICAL): Unpinned images — `src/config.rs`, `src/agent/claude_code/pod.rs`, `src/pipeline/executor.rs`

Add config fields with env var overrides:
```rust
runner_image: env_or("PLATFORM_RUNNER_IMAGE", "platform-runner:v1"),
git_clone_image: env_or("PLATFORM_GIT_CLONE_IMAGE", "alpine/git:2.47.2"),
kaniko_image: env_or("PLATFORM_KANIKO_IMAGE", "gcr.io/kaniko-project/executor:v1.23.2-debug"),
```
Update callsites: `pod.rs` resolve_image fallback, `pod.rs` git-clone init, `executor.rs` clone init, `trigger.rs` DEV_IMAGE_KANIKO.

### A6 (partial): Remove password from log — `src/store/bootstrap.rs`

Change line 324 from `tracing::warn!(...)` to `eprintln!(...)`. Keeps password visible in container stderr but out of observe pipeline.

### A22 (HIGH): Session cleanup shutdown — `src/main.rs`

Add `mut shutdown: tokio::sync::watch::Receiver<()>` param to `run_session_cleanup`. Use `tokio::select!` with `shutdown.changed()`. Pass `shutdown_tx.subscribe()` at spawn site.

### A23 (HIGH): MinIO access key redaction — `src/config.rs`

Change line 124: `.field("minio_access_key", &self.minio_access_key)` → `.field("minio_access_key", &"[REDACTED]")`

### A25 (HIGH): .expect() on fallible DB — `src/git/smart_http.rs`

Replace `.expect("token match implies user exists")` (~line 151) with `.ok_or(ApiError::Unauthorized)?`

---

## Verification

After each batch:
```bash
just test-unit          # ~3s, catches compile errors and logic regressions
```

After all 4 batches:
```bash
just ci-full            # fmt + lint + deny + test-unit + test-integration + test-e2e + build
```

Key tests to watch:
- Auth integration tests (`tests/auth_integration.rs`) — A1, A2, A7, A8
- Registry integration tests (`tests/registry_integration.rs`) — A8, A14
- Pipeline trigger tests (`tests/pipeline_trigger_integration.rs`) — A18
- Branch protection tests (`tests/branch_protection_integration.rs`) — A24

## Files Modified (27 files)

| Batch | Files |
|-------|-------|
| 1 | `src/api/projects.rs`, `src/api/merge_requests.rs`, `src/observe/alert.rs`, `src/git/smart_http.rs`, `src/git/lfs.rs`, `src/git/ssh_server.rs`, `src/registry/mod.rs`, `src/registry/auth.rs` |
| 2 | `src/api/webhooks.rs`, `src/deployer/reconciler.rs`, `src/pipeline/executor.rs`, `src/agent/claude_code/pod.rs`, `src/pipeline/trigger.rs`, `src/observe/query.rs`, `src/validation.rs` |
| 3 | `src/agent/pubsub_bridge.rs`, `src/registry/blobs.rs`, `src/git/browser.rs`, `src/observe/query.rs` |
| 4 | `src/config.rs`, `src/agent/claude_code/pod.rs`, `src/pipeline/executor.rs`, `src/pipeline/trigger.rs`, `src/store/bootstrap.rs`, `src/main.rs`, `src/git/smart_http.rs` |
