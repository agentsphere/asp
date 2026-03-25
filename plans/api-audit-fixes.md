# Plan: API Audit Fixes

## Context

The API design audit (`plans/api-audit-2026-03-25.md`) identified 35 findings across the platform's ~120 API endpoints. Key themes: inconsistent DELETE responses, unused `Validation` error variant, missing CRUD endpoints, bare `Vec<T>` list responses, and DateTime serialization inconsistency.

This plan addresses all findings in 5 atomic PRs, ordered by dependency. Each PR is independently mergeable. No migrations are needed — all changes are handler-level code, response types, and test updates.

## Design Principles

- **Mechanical consistency over cleverness** — most fixes are search-and-replace patterns applied uniformly across all handlers
- **Fix tests alongside handlers** — every handler change gets a corresponding test assertion update in the same PR
- **UI is already resilient** — `ui/src/lib/api.ts:29` already handles 204 No Content; no UI breaking changes expected
- **MCP servers need minor updates** — `platform-admin.js` must handle null responses from DELETE endpoints
- **No schema changes** — all fixes are in the application layer (handlers, types, tests)

---

## PR 1: Standardize DELETE Responses to 204 No Content

Addresses: **API1, API11** (CRITICAL + HIGH)

All DELETE/remove/revoke/deactivate handlers must return `StatusCode::NO_CONTENT` with empty body. Currently 9 handlers return `Json(json!({"ok": true}))`, 3 return `StatusCode::OK`, and 7 already return `StatusCode::NO_CONTENT`.

- [x] Types & errors defined (N/A — no new types)
- [x] Migration applied (N/A)
- [x] Tests written (red phase — assertion updates)
- [x] Implementation complete (green phase)
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

> **Deviation:** Found 3 additional DELETE handlers not in original plan: `delete_global_secret`, `delete_workspace_secret` (secrets.rs), `delete_project` (projects.rs), `delete_alert` (observe/alert.rs). Fixed all. Total: 14 handlers changed (not 11).
> Also found 25 test assertions across 13 files (not ~20 across 8).

### Code Changes

**Pattern: Replace `Json(json!({"ok": true}))` → `StatusCode::NO_CONTENT`**

For each handler, change return type from `Result<Json<serde_json::Value>, ApiError>` to `Result<StatusCode, ApiError>` and return `Ok(StatusCode::NO_CONTENT)`.

| File | Handler | Current | Change |
|---|---|---|---|
| `src/api/users.rs:594` | `deactivate_user` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/users.rs:785` | `revoke_api_token` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/admin.rs:373` | `remove_role` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/admin.rs:463` | `revoke_delegation_handler` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/admin.rs:663` | `deactivate_service_account` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/secrets.rs:313` | `delete_project_secret` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/webhooks.rs:363` | `delete_webhook` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/deployments.rs:1278` | `delete_ops_repo` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |
| `src/api/passkeys.rs:311` | `delete_passkey` | `Json(json!({"ok":true}))` | `StatusCode::NO_CONTENT` |

**Pattern: Replace `StatusCode::OK` → `StatusCode::NO_CONTENT`**

| File | Handler | Current | Change |
|---|---|---|---|
| `src/api/ssh_keys.rs` | `delete_ssh_key` | `StatusCode::OK` | `StatusCode::NO_CONTENT` |
| `src/api/gpg_keys.rs` | `delete_gpg_key` | `StatusCode::OK` | `StatusCode::NO_CONTENT` |

**Already correct (no change):** `workspaces.rs` (delete_workspace, remove_member), `cli_auth.rs` (delete_credentials), `flags.rs` (delete_flag), `commands.rs` (delete_command), `branch_protection.rs` (delete_protection), `llm_providers.rs` (delete_provider), `releases.rs` (delete_release)

**MCP update:**

| File | Change |
|---|---|
| `mcp/servers/platform-admin.js:230` | Handle null response: `data ? JSON.stringify(data, null, 2) : "Deleted successfully"` |
| `mcp/servers/platform-admin.js:258` | Same pattern |
| `mcp/servers/platform-admin.js:283` | Same pattern |

### Test Changes (~20 assertions across 8 files)

Every test asserting `StatusCode::OK` or `body["ok"]` for DELETE operations must be updated:

| Test File | Change |
|---|---|
| `tests/admin_integration.rs` | 4 DELETE assertions: `StatusCode::OK` → `StatusCode::NO_CONTENT`, remove `body["ok"]` checks |
| `tests/passkey_integration.rs` | 2 DELETE assertions: same pattern |
| `tests/auth_integration.rs` | 5+ DELETE assertions: same pattern |
| `tests/project_integration.rs` | 1 DELETE assertion: same pattern |
| `tests/ssh_server_integration.rs` | 1 DELETE assertion: same pattern |
| `tests/contract_integration.rs` | 1 DELETE assertion: same pattern |
| `tests/ssh_keys_integration.rs` | `StatusCode::OK` → `StatusCode::NO_CONTENT` |
| `tests/gpg_keys_integration.rs` | `StatusCode::OK` → `StatusCode::NO_CONTENT` |

Note: `tests/helpers/mod.rs::delete_json()` returns `(StatusCode, Value)` — when body is empty, `Value` is `Null`. Tests must stop asserting on body content.

### Test Outline — PR 1

**New behaviors to test:** None — existing tests cover delete operations; they just need assertion updates.

**Existing tests affected:** ~20 assertions across 8 test files (see table above).

**Estimated test count:** 0 new + ~20 assertion updates

### Verification
- `just test-unit` passes (no handler signature breaks)
- All integration tests with DELETE operations pass with `StatusCode::NO_CONTENT`
- UI DELETE operations work (frontend already handles 204)

---

## PR 2: Error Quality Improvements

Addresses: **API8** (HIGH), **API14** (HIGH), **API24** (MEDIUM), **API26** (MEDIUM), **API4** (CRITICAL — partial)

Three changes: (1) fix `parse_user_type` to return BadRequest instead of Internal, (2) standardize 404 messages for private resources, (3) document the permission denial pattern (read→404, write→403).

Note: API8 (migrating all validation to 422/`Validation`) is deferred to a follow-up. The `Validation` variant is designed for multi-field form submissions; single-field validators returning `BadRequest` with the field name is acceptable for a REST API. The current pattern (`BadRequest("field: message")`) is consistent and parseable. Migrating 114 call sites provides marginal benefit vs. risk.

- [x] Types & errors defined (N/A)
- [x] Migration applied (N/A)
- [x] Tests written (red phase — N/A, existing tests)
- [x] Implementation complete (green phase)
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

### Code Changes

| File | Change | Status |
|---|---|---|
| `src/api/admin.rs:154-157` | `parse_user_type`: change `ApiError::Internal(e)` → `ApiError::BadRequest("invalid user_type".into())` | **Done** |
| `src/api/secrets.rs:124` | `require_secret_read`: verified returns `NotFound` (correct for read) | **Verified** |
| `src/api/secrets.rs:147` | `require_secret_write`: verified returns `Forbidden` (correct for write) | **Verified** |
| `src/api/sessions.rs:189` | `require_agent_run`: changed `NotFound` → `Forbidden` (used only for create_session, a write op) | **Done** |

**404 message standardization** — audit all `NotFound` messages. The convention documented in CLAUDE.md:
- Read permission denied on private resources → `NotFound("project".into())` (leaks resource type but not ID — acceptable)
- The current pattern is already consistent enough. Only fix the write-path functions that incorrectly return 404.

### Test Outline — PR 2

**New behaviors to test:**
- `parse_user_type("invalid")` returns 400, not 500 — unit test
- Write permission denial returns 403, not 404 — integration test (if changed)

**Existing tests affected:**
- Any test calling the service-account create endpoint with bad `user_type` (currently expects 500, should expect 400)

**Estimated test count:** ~2 unit + ~1 integration update

### Verification
- `cargo nextest run --lib -E 'test(user_type)'` — parse error returns 400
- Session write permission denial returns 403

---

## PR 3: Response Envelope & Pagination Consistency

Addresses: **API6, API7** (HIGH), **API12** (HIGH), **API13** (MEDIUM), **API15** (MEDIUM), **API33** (LOW), **API32** (LOW)

Wrap bare `Vec<T>` list responses in `ListResponse<T>`, add pagination to releases, fix DateTime serialization in workspaces, fix `/api/users/list` route naming, fix passkey route prefix inconsistency.

- [x] Types & errors defined (N/A)
- [x] Migration applied (N/A)
- [x] Tests written (red phase — assertion updates)
- [x] Implementation complete (green phase)
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

> **Deviation:** Wrapped 16 handlers (not 10) — also found and fixed `list_roles`, `list_role_permissions`, `admin_list_ssh_keys`, `admin_list_gpg_keys`, `list_providers` (llm). All callers of `/api/users/list` and `/api/auth/passkey/login/*` updated across tests, MCP, and UI.

### Code Changes

**3a: Wrap bare Vec responses in ListResponse**

For each handler, change `Result<Json<Vec<T>>, ApiError>` to `Result<Json<ListResponse<T>>, ApiError>` and wrap: `Json(ListResponse { items, total: items.len() as i64 })`.

| File | Handler | Current Return |
|---|---|---|
| `src/api/sessions.rs` | `list_children` | `Json<Vec<SessionResponse>>` |
| `src/api/sessions.rs` | `list_iframes` | `Json<Vec<IframePanel>>` |
| `src/api/workspaces.rs` | `list_members` | `Json<Vec<MemberResponse>>` |
| `src/api/users.rs` | `list_api_tokens` | `Json<Vec<TokenResponse>>` |
| `src/api/admin.rs` | `list_delegations` | `Json<Vec<delegation::Delegation>>` |
| `src/api/passkeys.rs` | `list_passkeys` | `Json<Vec<...>>` |
| `src/api/ssh_keys.rs` | `list_ssh_keys` | `Json<Vec<...>>` |
| `src/api/gpg_keys.rs` | `list_gpg_keys` | `Json<Vec<...>>` |
| `src/api/user_keys.rs` | `list_provider_keys` | `Json<Vec<...>>` |
| `src/api/llm_providers.rs` | `list_llm_providers` | `Json<Vec<...>>` |

Each handler needs `use super::helpers::ListResponse;` (or verify it's already imported).

**3b: Add pagination to releases list**

| File | Change |
|---|---|
| `src/api/releases.rs` | Add `ListParams` query extraction to `list_releases`, add `LIMIT $N OFFSET $M` to SQL, add `COUNT(*)` query for total |

Pattern — same as `list_issues` in `src/api/issues.rs`:
```rust
async fn list_releases(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(params): Query<ListParams>,  // ADD THIS
) -> Result<Json<ListResponse<ReleaseResponse>>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);
    // ... add LIMIT/OFFSET to query, add COUNT(*) query
}
```

**3c: Fix DateTime serialization in workspaces**

| File | Change |
|---|---|
| `src/api/workspaces.rs:29-38` | `WorkspaceResponse`: change `created_at: String` → `created_at: DateTime<Utc>`, `updated_at: String` → `updated_at: DateTime<Utc>` |
| `src/api/workspaces.rs:40-52` | `From<Workspace>`: remove `.to_rfc3339()` calls, assign directly |
| `src/api/workspaces.rs:55-63` | `MemberResponse`: change `created_at: String` → `created_at: DateTime<Utc>` |
| `src/api/workspaces.rs:65-74` | `From<WorkspaceMember>`: remove `.to_rfc3339()`, assign directly |

Add `use chrono::{DateTime, Utc};` if not already imported (it is at line 1).

**3d: Fix `/api/users/list` → `GET /api/users`**

| File | Change |
|---|---|
| `src/api/users.rs:118-119` | Change `.route("/api/users", post(create_user))` + `.route("/api/users/list", get(list_users))` → `.route("/api/users", get(list_users).post(create_user))` |

This is safe because axum routes by HTTP method — GET and POST on the same path don't conflict. This matches the pattern used by every other resource (projects, issues, webhooks, etc.).

**3e: Fix passkey route prefix inconsistency**

| File | Change |
|---|---|
| `src/api/passkeys.rs:116-117` | Change `/api/auth/passkey/login/begin` → `/api/auth/passkeys/login/begin` |
| `src/api/passkeys.rs:117` | Change `/api/auth/passkey/login/complete` → `/api/auth/passkeys/login/complete` |
| `ui/src/lib/webauthn.ts` | Update login URLs to match |

### Test Changes

| Test File | Change |
|---|---|
| Tests asserting on bare arrays from list endpoints | Assert `body["items"]` is array, check `body["total"]` |
| `tests/workspace_integration.rs` | Update member list assertions if they check array directly |
| Tests calling `/api/users/list` | Change URL to `/api/users` (GET) |
| Tests calling `/api/auth/passkey/login/...` | Change to `/api/auth/passkeys/login/...` |
| Workspace test DateTime assertions | May need update if comparing exact string format |

### Test Outline — PR 3

**New behaviors to test:**
- Releases list respects `limit` and `offset` — integration
- Workspace response `created_at` is ISO 8601 DateTime (not double-quoted string) — integration

**Error paths to test:**
- `limit` > 100 is capped at 100 — unit or integration
- `offset` < 0 defaults to 0 — unit or integration

**Existing tests affected:**
- All tests hitting bare-Vec list endpoints (~10 endpoints)
- Tests hitting `/api/users/list` URL
- Tests hitting `/api/auth/passkey/login/...` URL

**Estimated test count:** ~2 new + ~15 assertion updates

### Verification
- All list endpoints return `{ "items": [...], "total": N }`
- GET `/api/users` returns user list (not 404)
- Passkey login flow works with new URL
- UI list pages render correctly (items/total structure unchanged)

---

## PR 4: CRUD Completeness — Issue/MR/Comment Deletion + Comment GET

Addresses: **API2, API3** (CRITICAL), **API5** (HIGH)

Add DELETE for issues/MRs (soft-close, not hard-delete) and add GET-single + DELETE for comments on both issues and MRs.

Design decision: Issues and MRs don't have `is_deleted` columns. Rather than adding a migration, we use **status-based closure**: `DELETE /issues/{number}` sets `status = 'closed'` (issues are already closeable). For MRs, `DELETE` sets `status = 'closed'`. This is semantically correct — "deleting" a tracking item means closing it. Hard deletion is reserved for projects (soft-delete with `is_active`). Comment deletion will use hard DELETE since comments have `ON DELETE CASCADE` and no audit requirement beyond the audit_log entry.

- [x] Types & errors defined (N/A — no new types)
- [x] Migration applied (N/A)
- [x] Tests written (red phase — tested via integration)
- [x] Implementation complete (green phase)
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

> **Note:** Used dynamic `sqlx::query()` for all new queries to avoid `.sqlx/` cache regeneration dependency. Follows test convention.

### Code Changes

**4a: Issue deletion (close) + comment GET/DELETE**

| File | Change |
|---|---|
| `src/api/issues.rs` router | Add `.delete(delete_issue)` to the `/{number}` route |
| `src/api/issues.rs` router | Add `.get(get_comment).delete(delete_comment)` to `/{comment_id}` route |
| `src/api/issues.rs` | New handler `delete_issue` — sets `status = 'closed'`, audit logs, returns 204 |
| `src/api/issues.rs` | New handler `get_comment` — fetch single comment by ID, returns `CommentResponse` |
| `src/api/issues.rs` | New handler `delete_comment` — hard DELETE, author or admin, audit log, returns 204 |

```rust
// delete_issue: close the issue
async fn delete_issue(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, number)): Path<(Uuid, i32)>,
) -> Result<StatusCode, ApiError> {
    require_project_write(&state, &auth, id).await?;
    // UPDATE issues SET status = 'closed', updated_at = now()
    // WHERE project_id = $1 AND number = $2
    // Audit log: action = "issue.delete"
    // fire_webhooks: event = "issue", action = "closed"
    Ok(StatusCode::NO_CONTENT)
}

// get_comment: retrieve single comment
async fn get_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, _number, comment_id)): Path<(Uuid, i32, Uuid)>,
) -> Result<Json<CommentResponse>, ApiError> {
    require_project_read(&state, &auth, id).await?;
    // SELECT * FROM comments WHERE id = $1 AND project_id = $2
    // Return CommentResponse
}

// delete_comment: remove comment (author or admin)
async fn delete_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, _number, comment_id)): Path<(Uuid, i32, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_project_write(&state, &auth, id).await?;
    // Verify comment exists AND (author_id = auth.user_id OR user is admin)
    // DELETE FROM comments WHERE id = $1
    // Audit log: action = "comment.delete"
    Ok(StatusCode::NO_CONTENT)
}
```

**4b: MR deletion (close) + comment GET/DELETE**

| File | Change |
|---|---|
| `src/api/merge_requests.rs` router | Add `.delete(delete_mr)` to the `/{number}` route |
| `src/api/merge_requests.rs` router | Add `.get(get_comment).delete(delete_comment)` to `/{comment_id}` route |
| `src/api/merge_requests.rs` | New handler `delete_mr` — sets `status = 'closed'`, audit, 204 |
| `src/api/merge_requests.rs` | New handler `get_comment` — same pattern as issues |
| `src/api/merge_requests.rs` | New handler `delete_comment` — same pattern |

**4c: State machine guard for MR deletion**

Only open MRs can be closed via DELETE. Already-merged MRs must not be closeable via DELETE:

```rust
// In delete_mr:
let mr = sqlx::query!("SELECT status FROM merge_requests WHERE ...")
    .fetch_optional(&state.pool).await?;
if mr.status == "merged" {
    return Err(ApiError::Conflict("cannot delete a merged merge request".into()));
}
```

### Test Outline — PR 4

**New behaviors to test:**
- DELETE issue sets status to closed — integration
- DELETE MR sets status to closed — integration
- DELETE merged MR returns 409 Conflict — integration
- GET single comment returns comment — integration
- DELETE comment by author succeeds — integration
- DELETE comment by non-author non-admin returns 403 — integration
- DELETE comment by admin succeeds — integration
- Webhook fires on issue/MR close via DELETE — integration

**Error paths to test:**
- DELETE non-existent issue returns 404 — integration
- DELETE on project without write access returns 403 — integration

**Estimated test count:** ~10 new integration tests

### Verification
- Issues: create → delete → verify status is 'closed'
- MRs: create → delete → verify closed; create → merge → delete → 409
- Comments: create → GET → DELETE → GET returns 404
- Webhook fires on close

---

## PR 5: CRUD Completeness — GET-Single Endpoints + Roles CRUD

Addresses: **API9, API10** (HIGH), **API16-API31** (MEDIUM — 16 findings)

Adds missing GET-single endpoints and completes roles CRUD. These are all mechanical: each is a simple `SELECT ... WHERE id = $1` handler with auth checks.

- [ ] Types & errors defined
- [ ] Migration applied (N/A)
- [ ] Tests written (red phase)
- [ ] Implementation complete (green phase)
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

### Code Changes

**5a: Roles — GET single, UPDATE, DELETE**

| File | Change |
|---|---|
| `src/api/admin.rs` router | Add `get(get_role)` to `/api/admin/roles/{id}` (new route) |
| `src/api/admin.rs` router | Add `.patch(update_role).delete(delete_role)` to same route |
| `src/api/admin.rs` | New `get_role` — SELECT by ID, return `RoleResponse` |
| `src/api/admin.rs` | New `update_role` — UPDATE name/description (reject if `is_system = true`), return `RoleResponse` |
| `src/api/admin.rs` | New `delete_role` — DELETE (reject if `is_system = true` or assigned to users), return 204 |
| `src/api/admin.rs` | New `UpdateRoleRequest { name: Option<String>, description: Option<String> }` |

System role guard:
```rust
if role.is_system {
    return Err(ApiError::Conflict("cannot modify system role".into()));
}
```

Assignment guard for delete:
```rust
let assigned = sqlx::query_scalar!("SELECT COUNT(*) FROM user_roles WHERE role_id = $1", role_id)
    .fetch_one(&state.pool).await?;
if assigned > 0 {
    return Err(ApiError::Conflict("role is still assigned to users".into()));
}
```

**5b: MR Reviews — GET single**

| File | Change |
|---|---|
| `src/api/merge_requests.rs` router | Add new route `/api/projects/{id}/merge-requests/{number}/reviews/{review_id}` with `get(get_review)` |
| `src/api/merge_requests.rs` | New `get_review` — SELECT by ID, require_project_read, return `ReviewResponse` |

**5c: Missing GET-single endpoints (batch)**

Each follows the same pattern: add `.get(get_X)` to the existing `/{id}` route (or create new route), implement handler with auth check + SELECT by ID.

| File | Resource | Route | New Handler |
|---|---|---|---|
| `src/api/admin.rs` | Delegations | `/api/admin/delegations/{id}` (new route) | `get_delegation` |
| `src/api/admin.rs` | Service Accounts | `/api/admin/service-accounts/{id}` (add GET) | `get_service_account` |
| `src/api/ssh_keys.rs` | SSH Keys | `/api/users/me/ssh-keys/{id}` (new route) | `get_ssh_key` |
| `src/api/passkeys.rs` | Passkeys | `/api/auth/passkeys/{id}` (add GET) | `get_passkey` |
| `src/api/user_keys.rs` | Provider Keys | `/api/users/me/provider-keys/{provider}` (new route) | `get_provider_key` |
| `src/api/llm_providers.rs` | LLM Providers | `/api/users/me/llm-providers/{id}` (new route) | `get_llm_provider` |
| `src/api/commands.rs` | Commands | `/api/commands/{id}` (add GET) | `get_command` |
| `src/api/users.rs` | API Tokens | `/api/tokens/{id}` (add GET) | `get_api_token` |

Each handler follows this template:
```rust
async fn get_X(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<XResponse>, ApiError> {
    // Auth check (require_admin for admin resources, owner check for user resources)
    let row = sqlx::query!("SELECT ... FROM table WHERE id = $1", id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("resource".into()))?;
    // For user-scoped resources: verify row.user_id == auth.user_id
    Ok(Json(XResponse { ... }))
}
```

**5d: Workspace member role update**

| File | Change |
|---|---|
| `src/api/workspaces.rs` router | Add `.patch(update_member)` to `/api/workspaces/{id}/members/{member_id}` (new route alongside DELETE) |
| `src/api/workspaces.rs` | New `UpdateMemberRequest { role: String }`, new `update_member` handler |

**5e: Session DELETE endpoint**

| File | Change |
|---|---|
| `src/api/sessions.rs` router | Add `.delete(delete_session)` to `/api/projects/{id}/sessions/{session_id}` |
| `src/api/sessions.rs` | New `delete_session` — sets `status = 'stopped'` if running, audit log, returns 204 |

### Test Outline — PR 5

**New behaviors to test:**
- GET single role returns role — integration
- UPDATE role updates name/description — integration
- UPDATE system role returns 409 — integration
- DELETE role with no assignments succeeds — integration
- DELETE role with assignments returns 409 — integration
- GET single review returns review — integration
- GET single delegation, service account, SSH key, etc. — integration (1 per resource)
- Update workspace member role — integration
- DELETE session sets status to stopped — integration

**Error paths to test:**
- GET non-existent resource returns 404 — integration
- GET resource owned by different user returns 404/403 — integration

**Estimated test count:** ~20 new integration tests

### Verification
- Each new GET-single endpoint returns the resource
- Role CRUD lifecycle: create → get → update → delete
- Workspace member: add → update role → verify
- Session: create → delete → verify stopped

---

## Deferred Items (Backlog)

These findings have low severity or high effort-to-value ratio:

| Finding | Reason Deferred |
|---|---|
| **API8**: Migrate all validation to 422 | 114 call sites, marginal benefit — current `BadRequest("field: msg")` is consistent and parseable |
| **API16**: Release assets list/update/delete | Feature addition, not consistency fix |
| **API18**: Deploy targets update/delete | Feature addition |
| **API25**: Secret request reject/cancel | Feature addition |
| **API27**: Notifications PATCH URL | Low severity, verb-in-URL is acceptable for action endpoints |
| **API29**: Dynamic sqlx error handling | Low risk in practice — constraint violations are rare on these tables |
| **API30**: Global/workspace secrets GET-single | Low usage endpoints |
| **API34**: Sort parameter on lists | Feature addition |
| **API35**: API versioning | Architectural decision, not a bug fix |

---

## Cascading Impact Summary

| System | Impact | PR |
|---|---|---|
| **Integration tests** | ~20 DELETE assertion updates, ~15 list assertion updates, ~30 new tests | PR 1, 3, 4, 5 |
| **E2E tests** | May need URL updates for `/api/users` and passkey login | PR 3 |
| **UI** | Already resilient to 204; passkey login URL update needed | PR 3 |
| **MCP servers** | `platform-admin.js` null response handling | PR 1 |
| **TypeScript types** | Auto-generated via `ts-rs` — rebuild with `just ui` after response type changes | PR 3 |
| **`.sqlx/` cache** | No changes — no new compile-time queries in PRs 1-3; PRs 4-5 add queries → `just db-prepare` | PR 4, 5 |
