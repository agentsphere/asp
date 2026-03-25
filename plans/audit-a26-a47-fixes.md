# Plan: Fix Audit Findings A26–A47

## Context

These are the 22 MEDIUM findings from the codebase audit. They span cache invalidation, missing audit entries, input validation gaps, tracing instrumentation, stale dead_code annotations, and defensive coding. Most are 1-5 line surgical fixes.

## Design Principles

- **Pattern alignment** — follow existing canonical patterns from established modules
- **Batch by domain** — group related fixes to minimize PR count
- **Test every behavioral change** — validation/auth changes get integration tests; code quality changes (tracing, dead_code, .expect) need only compile

---

## PR 1: Cache Invalidation & Session Cleanup (A26, A29, A47)

Fixes permission cache not invalidated after role permission changes, and service account deactivation not cleaning up sessions.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/api/admin.rs` (after `set_role_permissions` tx.commit, ~line 293) | Query all users with this role: `let affected_users: Vec<Uuid> = sqlx::query_scalar("SELECT user_id FROM user_roles WHERE role_id = $1").bind(id).fetch_all(&state.pool).await.unwrap_or_default();` Then invalidate each: `for uid in affected_users { let _ = resolver::invalidate_permissions(&state.valkey, uid, None).await; }` |
| `src/api/admin.rs` (`deactivate_service_account`, ~line 675) | Add `sqlx::query("DELETE FROM auth_sessions WHERE user_id = $1").bind(id).execute(&state.pool).await?;` before the `api_tokens` delete |
| `src/rbac/resolver.rs` (A47 — doc-only) | Add comment near `cache_ttl()`: `// For emergency revocations, call invalidate_permissions() directly — do not rely on TTL expiry alone.` Already documented; no code change needed. |

### Test Outline

**New tests (integration):**
| Test | File | What it asserts |
|---|---|---|
| `set_role_permissions_invalidates_cache` | `tests/admin_integration.rs` | Assign role to user, verify access. Change role permissions (remove the permission). Verify user's next request reflects new permissions (no stale cache). |
| `deactivate_service_account_deletes_sessions` | `tests/admin_integration.rs` | Create SA, login (create session), deactivate SA, verify session is gone (query auth_sessions). |

**Existing tests to update:** None — existing tests don't assert on cache/session state after permission changes.

---

## PR 2: Missing Audit Logging (A27, A35)

Adds write_audit to passkey rename, MR comment creation, and disable_auto_merge.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/api/passkeys.rs` (`rename_passkey`, ~line 290) | Add `write_audit(&state.pool, &AuditEntry { actor_id: auth.user_id, actor_name: &auth.user_name, action: "auth.passkey_rename", resource: "passkey", resource_id: Some(id), project_id: None, detail: Some(serde_json::json!({"name": body.name})), ip_addr: auth.ip_addr.as_deref() }).await;` before the Ok return |
| `src/api/merge_requests.rs` (`create_comment`, ~line 945) | Add `write_audit(...)` with action `"comment.create"`, resource `"merge_request"`, resource_id `Some(mr_id)`, project_id `Some(id)` |
| `src/api/merge_requests.rs` (`disable_auto_merge`, ~line 1370) | Add `write_audit(...)` with action `"mr.auto_merge.disable"`, resource `"merge_request"`, project_id `Some(id)` |

### Test Outline

**New tests (integration):**
| Test | File | What it asserts |
|---|---|---|
| `rename_passkey_writes_audit` | `tests/passkey_integration.rs` | After rename, query `audit_log WHERE action = 'auth.passkey_rename'` returns 1 row |
| `mr_comment_writes_audit` | `tests/issue_mr_integration.rs` | After creating MR comment, query `audit_log WHERE action = 'comment.create'` returns 1 row |

---

## PR 3: Input Validation Gaps (A28, A32, A34, A46)

Fixes passkey rate limit key, flag evaluate key count, missing validations across newer modules, and release asset filename sanitization.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

#### A28: Passkey rate limit fix
| File | Change |
|---|---|
| `src/api/passkeys.rs` (~line 354) | Change rate limit identifier from `&body.challenge_id` to IP-based: extract client IP from request headers or use `"global"` as a fallback. Best approach: use the peer IP. Since `complete_login` doesn't take `AuthUser` (unauthenticated endpoint), extract IP from request extensions or use a fixed key per source. Simplest: `"global"` with a higher limit (50/300s), or pass the IP through. |

Actually, `complete_login` doesn't have access to the request's IP. The simplest effective fix: change identifier to `"global"` with limit 50/300s (50 attempts per 5 minutes globally). This prevents brute force while allowing legitimate multi-device logins.

| File | Change |
|---|---|
| `src/api/passkeys.rs` (~line 354) | Change `&body.challenge_id` to `"global"` and increase limit from 10 to 50: `check_rate(&state.valkey, "passkey_login", "global", 50, 300).await?;` |

#### A32: Flag evaluate key count
| File | Change |
|---|---|
| `src/api/flags.rs` (`evaluate_flags`, ~line 755) | Add before the loop: `if body.keys.len() > 100 { return Err(ApiError::BadRequest("too many keys (max 100)".into())); }` |

#### A34: Missing validations
| File | Change |
|---|---|
| `src/api/pipelines.rs` (`trigger_pipeline`, ~line 148) | Add `validation::check_branch_name(&body.git_ref)?;` before calling `on_api` |
| `src/api/deployments.rs` (`create_release`, ~line 460) | Add `validation::check_length("image_ref", &body.image_ref, 1, 500)?;` |
| `src/api/llm_providers.rs` (`create_provider`, ~line 112) | Add `validation::check_length("provider_type", &body.provider_type, 1, 100)?;` |
| `src/api/llm_providers.rs` (`create_provider`, ~line 113) | Add `if body.env_vars.len() > 50 { return Err(ApiError::BadRequest("too many env_vars (max 50)".into())); }` and `for (k, v) in &body.env_vars { validation::check_length("env_var key", k, 1, 255)?; validation::check_length("env_var value", v, 0, 10_000)?; }` |
| `src/api/llm_providers.rs` (`update_provider`, same pattern) | Same env_vars validation |

#### A46: Release asset filename sanitization
| File | Change |
|---|---|
| `src/api/releases.rs` (`download_asset`, ~line 472) | Replace `format!("attachment; filename=\"{name}\"")` with: `let safe_name = name.replace(['\"', '\\', '\r', '\n', '/', '\0'], "_"); format!("attachment; filename=\"{safe_name}\"")` |

### Test Outline

**New tests (unit):**
| Test | File | What it asserts |
|---|---|---|
| `release_filename_sanitized` | `src/api/releases.rs` (or unit test) | Filename with `"quotes"` and `path/sep` is sanitized to safe chars |

**New tests (integration):**
| Test | File | What it asserts |
|---|---|---|
| `evaluate_flags_too_many_keys_rejected` | `tests/deployment_integration.rs` | Request with 101 keys returns 400 |
| `trigger_pipeline_invalid_git_ref` | `tests/pipeline_integration.rs` | `git_ref` containing `..` returns 400 |
| `create_release_empty_image_ref_rejected` | `tests/deployment_integration.rs` | Empty `image_ref` returns 400 |

**Existing tests to update:** None.

---

## PR 4: Authorization Hardening (A30, A33, A37, A39, A40, A41)

Fixes list_projects RBAC gap, global command access, setup_commands validation in pod path, gateway weight validation, workspace delete cascade, and LLM provider ownership.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

#### A30: list_projects RBAC-granted private projects
| File | Change |
|---|---|
| `src/api/projects.rs` (`list_projects` WHERE clause, ~line 505) | Add a subquery to the visibility filter: `OR (visibility = 'private' AND EXISTS(SELECT 1 FROM user_roles ur JOIN role_permissions rp ON rp.role_id = ur.role_id JOIN permissions p ON p.id = rp.permission_id WHERE ur.user_id = $4 AND p.name = 'project:read' AND (ur.project_id = projects.id OR ur.project_id IS NULL)))`. Same for the COUNT query. This allows private projects where the user has project:read via a direct role to appear in the list. |

> **Note:** This is a complex SQL change. Alternative: use `require_project_read` as a post-filter, but that's N+1. The subquery approach is better for performance.

#### A33: Global command access control
| File | Change |
|---|---|
| `src/api/commands.rs` (`get_command`, ~line 430) | Add an `else` branch after the workspace check: `else { super::helpers::require_admin(&state, &auth).await?; }` — global commands require admin. |

#### A37: Setup commands validation in pod path
| File | Change |
|---|---|
| `src/agent/claude_code/pod.rs` (~line 735, before joining commands) | Add: `if let Some(ref cmds) = params.config.setup_commands { crate::validation::check_setup_commands(cmds).map_err(|e| AgentError::PodCreationFailed(e.to_string()))?; }` |

#### A39: Gateway weight validation
| File | Change |
|---|---|
| `src/deployer/gateway.rs` (`build_weighted_httproute`, ~line 22) | Add at function start: `debug_assert!(stable_weight + canary_weight == 100, "weights must sum to 100: {stable_weight} + {canary_weight}");` Also add a runtime check: `if stable_weight + canary_weight != 100 { tracing::error!(stable_weight, canary_weight, "gateway weights do not sum to 100"); }` |

#### A40: Workspace soft-delete cascade
| File | Change |
|---|---|
| `src/workspace/service.rs` (`delete_workspace`, ~line 176) | After soft-deleting the workspace, add: `sqlx::query!("UPDATE projects SET is_active = false, updated_at = now() WHERE workspace_id = $1 AND is_active = true", id).execute(pool).await?;` |

#### A41: LLM provider ownership check
| File | Change |
|---|---|
| `src/secrets/llm_providers.rs` (`update_validation_status`, ~line 321) | Add `user_id: Uuid` parameter. Change SQL to `WHERE id = $1 AND user_id = $3`. Update the single call site in `src/agent/llm_validate.rs` to pass the user_id. |

### Test Outline

**New tests (unit):**
| Test | File | What it asserts |
|---|---|---|
| `gateway_weights_must_sum_to_100` | `src/deployer/gateway.rs` | `build_weighted_httproute` with weights (80, 30) triggers debug_assert in debug mode |

**New tests (integration):**
| Test | File | What it asserts |
|---|---|---|
| `list_projects_shows_rbac_granted_private` | `tests/project_integration.rs` | User with project:read role on a private project sees it in list |
| `get_global_command_requires_admin` | `tests/commands_integration.rs` | Non-admin cannot GET a global command (403) |
| `delete_workspace_cascades_to_projects` | `tests/workspace_integration.rs` | After deleting workspace, its projects are also inactive |

**Existing tests to update:**
| Test | File | Change |
|---|---|---|
| Any test that expects non-admin to read global commands | `tests/commands_integration.rs` | Verify `list_commands_returns_global` still works (listing may differ from getting) |

---

## PR 5: Parquet Rotation Safety (A36)

Adds verification after MinIO upload before deleting from Postgres.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/observe/parquet.rs` (`upload_and_delete_logs`, ~line 127) | After `state.minio.write(&path, parquet_bytes).await?;`, add verification: `let stat = state.minio.stat(&path).await.map_err(|e| { tracing::error!(error = %e, %path, "parquet upload verification failed"); e })?;` This ensures the file was actually written before deleting from Postgres. If stat fails, the `?` returns early and the DELETE is skipped. |

### Test Outline

No new tests — the upload verification is a defense-in-depth check. The existing observe integration tests cover the rotation path. A unit test would require mocking MinIO which is complex for minimal value.

---

## PR 6: Response Builder .expect() + Pipeline Git Ref (A45)

Replaces 6 `.unwrap()` on Response::builder().body() with `.expect()`.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/api/pipelines.rs` lines 375-378, 380-383, 405-408, 409-412, 415-418, 498-508 | Replace all 6 `.unwrap()` with `.expect("infallible: Response builder with valid headers")` |

### Test Outline

No new tests — these are infallible operations. Existing pipeline tests cover the handlers.

---

## PR 7: Tracing Instrumentation (A43)

Adds `#[tracing::instrument]` to ~36 uninstrumented async functions in flags and deployments.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/api/flags.rs` | Add `#[tracing::instrument(skip(state, body), err)]` (or `skip(state)` for handlers without body) to all 18 async functions. Use `fields(%project_id)` where project_id is available as a path param. For `evaluate_single` and `evaluate_single_uncached`, use `skip(pool, valkey)`. |
| `src/api/deployments.rs` | Add `#[tracing::instrument(skip(state, body), err)]` to all 20 uninstrumented async functions. The 4 already-instrumented ops_repo functions are left unchanged. |

### Test Outline

No new tests — tracing instrumentation doesn't change behavior. Verified by compilation.

---

## PR 8: Dead Code Cleanup (A44)

Removes ~30 stale `#[allow(dead_code)]` annotations from fully-implemented modules.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/store/mod.rs:18` | Remove `#[allow(dead_code)]` — all AppState fields consumed |
| `src/error.rs:5` | Remove `#[allow(dead_code)]` — all ApiError variants used |
| `src/config.rs:5` | Remove `#[allow(dead_code)]` — all Config fields consumed |
| `src/notify/mod.rs:2,4,6` | Remove 3 `#[allow(dead_code)]` — all notify modules used |
| `src/secrets/mod.rs:2,6` | Remove `#[allow(dead_code)]` — all secrets modules used |
| `src/workspace/mod.rs:1` | Remove — workspace module wired in |
| `src/agent/provider.rs:197,201` | Remove — marked "Pending removal in Step 6" |
| `src/agent/claude_code/mod.rs:3` | Remove — marked "Pending removal in Step 6" |
| `src/deployer/error.rs:4` | Remove — deployer fully implemented |
| `src/observe/error.rs:4` | Remove — observe fully implemented |
| `src/pipeline/error.rs:4` | Remove — pipeline fully implemented |

After removing, run `cargo check` to verify no genuine dead code warnings appear. If any do, either use the code or delete it (don't re-add the allow).

### Test Outline

No new tests — compilation-only change. `cargo check --all-targets` verifies no warnings.

---

## PR 9: Agent Security Hardening (A38)

Adds seccompProfile to agent pod containers.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change |
|---|---|
| `src/agent/claude_code/pod.rs` (`main_container_security`, line 30) | Add `seccomp_profile: Some(SeccompProfile { type_: "RuntimeDefault".into(), ..Default::default() })` to the SecurityContext. Keep `allow_privilege_escalation: true` (needed for sudo). |
| `src/agent/claude_code/pod.rs` (pod-level security context, ~line 227) | Add `seccomp_profile: Some(SeccompProfile { type_: "RuntimeDefault".into(), ..Default::default() })` to the pod-level `PodSecurityContext`. |

### Test Outline

**Existing tests to update:**
| Test | File | Change |
|---|---|---|
| `pipeline_step_container_has_security_context` or similar | `src/pipeline/executor.rs` tests | Verify seccomp profile is set in pod spec (if agent pod tests exist) |

---

## Summary

| PR | Findings | Files Changed | New Tests | Risk |
|---|---|---|---|---|
| PR 1: Cache & Sessions | A26, A29, A47 | 2 | 2 integration | Low |
| PR 2: Audit Logging | A27, A35 | 2 | 2 integration | Low |
| PR 3: Input Validation | A28, A32, A34, A46 | 5 | 3 integration + 1 unit | Low |
| PR 4: Auth Hardening | A30, A33, A37, A39, A40, A41 | 6 | 1 unit + 3 integration | Medium |
| PR 5: Parquet Safety | A36 | 1 | 0 | Low |
| PR 6: .expect() | A45 | 1 | 0 | Zero |
| PR 7: Tracing | A43 | 2 | 0 | Zero |
| PR 8: Dead Code | A44 | ~11 | 0 | Low |
| PR 9: Agent Security | A38 | 1 | 0-1 | Low |
| **Total** | **A26-A47** | **~31 files** | **~12 tests** | |

### Recommended merge order

1. **PR 6** (.expect) — zero risk
2. **PR 8** (dead_code) — low risk, cleanup
3. **PR 7** (tracing) — zero behavioral change
4. **PR 2** (audit logging) — simple additions
5. **PR 1** (cache invalidation) — important correctness fix
6. **PR 3** (input validation) — security hardening
7. **PR 5** (parquet safety) — defense in depth
8. **PR 9** (agent seccomp) — security hardening
9. **PR 4** (auth hardening) — most complex, largest blast radius

### Findings NOT addressed (deferred)

- **A31** (auto-merge synthetic AuthUser scope bypass) — design decision needed on whether to store original scopes; low practical risk since auto-merge only triggers on the same project
- **A42** (Valkey KEYS→SCAN) — performance optimization for scale; current key space is small; backlog
