# Plan: Test Audit Fixes — CRITICAL + HIGH Findings

**Source:** `plans/test-audit-2026-03-25.md` (59 findings, 25 CRITICAL+HIGH)
**Scope:** All 9 CRITICAL + 16 HIGH findings
**Approach:** 6 phases, ordered by blast radius (false positives first, then new coverage, then flakiness)

## Progress

- [x] Phase 1: Fix False Positives & Assertion Bugs (T4–T6, T10–T11, T21, T23–T24)
- [x] Phase 2: New Integration Tests for Untested API Modules (T1, T22)
  - `tests/releases_integration.rs` — 10 tests for 7 handlers
  - `tests/branch_protection_integration.rs` — 9 tests for 5 handlers
  - `tests/flags_integration.rs` — 16 tests for 12 handlers
  - `tests/llm_providers_integration.rs` — 8 tests for 7 handlers
  - `tests/onboarding_integration.rs` — 12 tests for 10+ handlers
- [x] Phase 3: Security-Critical Coverage (T7–T9, T13–T14, T15, T25)
  - `tests/validation_integration.rs` — input validation + pagination + token expiry + soft-delete
  - `tests/secrets_integration.rs` — 3 new tests for decryption round-trip
  - Merge gates (T2) and auto-merge (T3) deferred to follow-up (requires git repo setup)
- [x] Phase 4: RBAC & Data Integrity (T16)
  - `tests/workspace_integration.rs` — 2 new workspace-derived permission tests
- [x] Phase 5: Fix Flaky Tests (T17–T20)
  - Observe ingest: removed 4× sleep(1500ms), use shutdown-drain
  - Webhook dispatch: removed 3× sleep(3s), polling loop for concurrent test
  - Deployment preview: removed sleep(2s), inline await already completes
  - Auth token: replaced sleep(200ms) with polling loop
- [x] Phase 6: Infrastructure & Remaining HIGH Items (T12, T23)
  - Webhook payload content verification added
  - Permission cache TTL set in both test_state() and e2e_state()

### Deferred to follow-up
- T2: `enforce_merge_gates()` tests — requires branch protection + git repo + review setup
- T3: auto_merge enable/disable — requires MR with open status
- T9: Rate limit boundary tests — requires Valkey counter manipulation

---

## Phase 1: Fix False Positives & Assertion Bugs (T4–T6, T10–T11, T21)

Quick wins — existing tests that pass with broken code. No new test files needed.

### Step 1.1 — Fix `body_json` silent null (T21)

**File:** `tests/helpers/mod.rs:665-671`
**Also:** `tests/e2e_helpers/mod.rs:944`, `tests/auth_integration.rs:32,56`, `tests/e2e_gitops.rs:212`

Change in all 5 locations:
```rust
// Before:
serde_json::from_slice(&bytes).unwrap_or(Value::Null)
// After:
serde_json::from_slice(&bytes).expect("response body is not valid JSON")
```

This catches any test where the server returns non-JSON (HTML error page, empty 500, etc.) instead of silently producing `Value::Null` that causes confusing downstream assertion failures.

### Step 1.2 — Fix false positive: `alert_fired_sets_cooldown_on_attempt` (T4)

**File:** `tests/eventbus_integration.rs` (~line 803-841)

The test only asserts `result.is_ok()`. Add a Valkey cooldown key check after the handler runs:
```rust
// After result.is_ok():
let cooldown_key = format!("alert:cooldown:{}", alert_rule_id);
let exists: bool = state.valkey.exists(&cooldown_key).await.unwrap();
assert!(exists, "cooldown key should be set after critical alert");
```

Model after `alert_fired_warning_severity_proceeds` which already does this correctly.

### Step 1.3 — Fix false positive: `setup_creates_personal_workspace` (T5)

**File:** `tests/setup_integration.rs` (~line 181-206)

Change `_status` to `status` and assert:
```rust
let (status, body) = helpers::post_json(&app, &token, "/api/setup", &json!({...})).await;
assert_eq!(status, StatusCode::OK, "setup failed: {body}");
```

### Step 1.4 — Fix OR-assertion security violation (T6)

**File:** `tests/pipeline_integration.rs::private_project_returns_404_for_non_member` (~line 823-826)

```rust
// Before:
assert!(status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN, ...);
// After:
assert_eq!(status, StatusCode::NOT_FOUND, "private project should return 404, not 403 (avoids leaking existence)");
```

If this fails, it means the handler returns 403 for private projects — fix the handler too.

### Step 1.5 — Tighten `>= N` assertions to `== N` (T11)

**Files:** 8+ tests across `admin_integration.rs`, `issue_mr_integration.rs`, `project_integration.rs`, `rbac_integration.rs`

With `#[sqlx::test]` providing isolated DBs, exact counts are predictable:
- `admin_actions_create_audit_log`: `assert_eq!(row.0, 1)` (not `>= 1`)
- `admin_list_users`: `assert_eq!(total, 3)` (admin + 2 created)
- `list_issues`: `assert_eq!(items.len(), 3)` (3 created)
- `list_merge_requests`: `assert_eq!(items.len(), 2)`
- `list_projects_pagination`: `assert_eq!(total, 5)`
- `role_assignment_creates_audit`: `assert_eq!(row.0, 1)`
- `delegation_audit_logged`: `assert_eq!(create_audit.0, 1)` and `assert_eq!(revoke_audit.0, 1)`

Exception: E2E tests in `e2e_demo.rs` may legitimately use `>=` due to shared state. Leave those.

### Step 1.6 — Fix discarded response bodies on mutations (T10)

**Files:** 14 tests across `admin_integration.rs`, `rbac_integration.rs`, `webhook_integration.rs`, `dashboard_integration.rs`

For each test that discards the response body on a mutation:
- Bind `body` instead of `_body` or `_`
- Assert at least one field from the body (e.g., `assert!(body["ok"].as_bool().unwrap_or(false))`)
- For DELETE endpoints, assert status explicitly if currently unchecked

**Verification:** `just test-unit` after each file, then `just test-integration` at end of phase.

---

## Phase 2: New Integration Tests for Untested API Modules (T1, T22)

5 new test files covering ~40 handlers with zero tests.

### Step 2.1 — `tests/releases_integration.rs` (7 handlers)

Tests needed:
| Test name | Endpoint | Asserts |
|---|---|---|
| `create_release` | POST `/api/projects/{id}/releases` | 201, tag_name/name/body fields match, audit logged |
| `create_release_invalid_tag` | POST | 400 on empty/oversized tag_name |
| `list_releases` | GET `/api/projects/{id}/releases` | 200, items/total correct |
| `get_release_by_tag` | GET `/api/projects/{id}/releases/{tag}` | 200, fields match |
| `get_release_not_found` | GET | 404 for nonexistent tag |
| `update_release` | PATCH `/api/projects/{id}/releases/{tag}` | 200, updated fields |
| `delete_release` | DELETE `/api/projects/{id}/releases/{tag}` | 200, subsequent GET returns 404 |
| `upload_asset` | POST `/api/projects/{id}/releases/{tag}/assets` | 200, asset_id + filename in response |
| `download_asset` | GET `/api/projects/{id}/releases/{tag}/assets/{id}/download` | 200, bytes match uploaded |
| `release_requires_project_write` | POST/PATCH/DELETE | 404 for viewer role |
| `release_allows_project_read` | GET list/get/download | 200 for reader role |

**Notes:** Need `post_multipart` helper or inline multipart body construction. Assets stored in MinIO at `releases/{release_id}/{file_name}`. No webhook dispatch needed.

### Step 2.2 — `tests/branch_protection_integration.rs` (5 handlers)

Tests needed:
| Test name | Endpoint | Asserts |
|---|---|---|
| `create_protection` | POST `/api/projects/{id}/branch-protections` | 201, all boolean fields match |
| `create_protection_invalid_pattern` | POST | 400 on empty/oversized pattern |
| `create_protection_invalid_merge_methods` | POST | 400 on empty or invalid values |
| `list_protections` | GET | 200, items correct |
| `get_protection` | GET `/{rule_id}` | 200, fields match |
| `update_protection` | PATCH `/{rule_id}` | 200, partial update works |
| `delete_protection` | DELETE `/{rule_id}` | 200, subsequent GET returns 404 |
| `protection_requires_project_write` | all ops | 404 for viewer/reader roles |
| `protection_not_found` | GET/PATCH/DELETE non-existent | 404 |

**Notes:** All operations require `require_project_write` (even list/get — admin-level config). Valid merge methods: `merge`, `squash`, `rebase`.

### Step 2.3 — `tests/llm_providers_integration.rs` (7 handlers)

Tests needed:
| Test name | Endpoint | Asserts |
|---|---|---|
| `create_provider` | POST `/api/users/me/llm-providers` | 201, label/type match, env_vars NOT returned in response |
| `create_provider_no_master_key` | POST | 503 ServiceUnavailable when no master key |
| `list_providers` | GET | 200, items correct |
| `update_provider` | PUT `/{id}` | 200, label updated |
| `delete_provider` | DELETE `/{id}` | 200, gone from list |
| `validate_provider` | GET `/{id}/validate` | 200, returns SSE stream |
| `set_active_provider` | PUT `/api/users/me/active-provider` | 200 |
| `get_active_provider` | GET `/api/users/me/active-provider` | 200, matches set |
| `provider_scoped_to_user` | GET list as different user | 200 but empty (user B can't see user A's providers) |
| `create_provider_validates_env_vars` | POST with >50 env vars | 400 |

**Notes:** User-scoped (no project permissions). Requires `PLATFORM_MASTER_KEY` (already set in test helpers). `validate_provider` returns SSE — test at least status code + content-type.

### Step 2.4 — `tests/onboarding_integration.rs` (10+ handlers)

Tests needed:
| Test name | Endpoint | Asserts |
|---|---|---|
| `wizard_status_admin` | GET `/api/onboarding/wizard-status` | 200, `show_wizard` boolean |
| `wizard_status_non_admin` | GET | 200, `show_wizard: false` (non-admin always sees false) |
| `complete_wizard` | POST `/api/onboarding/wizard` | 200, wizard marked complete |
| `complete_wizard_non_admin` | POST | 403 |
| `get_settings` | GET `/api/onboarding/settings` | 200, settings object |
| `update_settings` | PATCH `/api/onboarding/settings` | 200, updated fields |
| `create_demo_project` | POST `/api/onboarding/demo-project` | 200, project created |
| `create_demo_project_non_admin` | POST | 403 |
| `verify_oauth_token_invalid` | POST `/api/onboarding/claude-auth/verify-token` | 400 or 500 (invalid token) |
| `verify_oauth_token_too_short` | POST | 400 (token < 10 chars) |

**Notes:** Most endpoints are admin-only. Claude OAuth flow (`start_claude_auth`, `submit_auth_code`, `cancel_claude_auth`) depends on external CLI binary — test error paths only (no real CLI in integration). `wizard_status` has special non-admin logic.

### Step 2.5 — `tests/flags_integration.rs` (12 handlers) (T22)

Tests needed:
| Test name | Endpoint | Asserts |
|---|---|---|
| `create_flag` | POST `/api/projects/{id}/flags` | 201, key/type/environment match |
| `create_flag_invalid_key` | POST | 400 on invalid key |
| `create_flag_duplicate_key` | POST | 409 Conflict |
| `list_flags` | GET | 200, items/total |
| `get_flag` | GET `/{key}` | 200, all fields |
| `update_flag` | PATCH `/{key}` | 200, description updated |
| `delete_flag` | DELETE `/{key}` | 200, gone from list |
| `toggle_flag` | POST `/{key}/toggle` | 200, enabled flipped |
| `add_rule` | POST `/{key}/rules` | 200, rule_id returned |
| `add_rule_invalid_type` | POST | 400 |
| `delete_rule` | DELETE `/{key}/rules/{id}` | 200 |
| `set_override` | PUT `/{key}/overrides/{user_id}` | 200 |
| `delete_override` | DELETE `/{key}/overrides/{user_id}` | 200 |
| `flag_history` | GET `/{key}/history` | 200, history entries for CRUD ops |
| `flag_requires_manage_permission` | POST/PATCH/DELETE/toggle | 404 for viewer role |
| `flag_allows_read` | GET list/get | 200 for reader role |

**Notes:** Uses `Permission::FlagManage`. Need to figure out which role grants this (likely `admin` or `maintainer`). Flags keyed by string `key` in path. Evaluation endpoint already tested in `deployment_integration.rs`. `invalidate_flag_cache` deletes Valkey cache key.

**Verification after Phase 2:** `just test-integration` targeting each new file individually via `just test-bin`.

---

## Phase 3: Security-Critical Coverage (T2, T3, T7–T9, T13–T14, T25)

### Step 3.1 — `enforce_merge_gates()` tests (T2)

**File:** `tests/issue_mr_integration.rs` (add new tests to existing file)

Prerequisite: branch_protection CRUD from Step 2.2 must work. These tests create protection rules then attempt merge.

Tests needed:
| Test name | Setup | Assert |
|---|---|---|
| `merge_blocked_insufficient_approvals` | Create protection rule requiring 2 approvals, create MR with 0 approvals | 400 on merge |
| `merge_allowed_with_sufficient_approvals` | Same rule, add 2 approve reviews | 200 on merge |
| `merge_blocked_wrong_method` | Protection rule allows only `squash`, attempt `merge` | 400 |
| `merge_blocked_no_ci_success` | Protection rule requires CI, no pipeline exists | 400 |
| `merge_admin_bypass` | Protection rule with `allow_admin_bypass=true`, admin user | 200 |
| `merge_admin_no_bypass` | Protection rule with `allow_admin_bypass=false`, admin user | 400 |

**Notes:** Each test needs: create project → seed bare repo → create source branch → create MR → create protection rule → attempt merge. Use `seed_bare_repo()` helper already in `issue_mr_integration.rs`. Protection rules target `target_branch` via pattern match.

### Step 3.2 — Auto-merge endpoints (T3)

**File:** `tests/issue_mr_integration.rs` (add new tests)

Tests needed:
| Test name | Endpoint | Assert |
|---|---|---|
| `enable_auto_merge` | PUT `/api/projects/{id}/merge-requests/{n}/auto-merge` | 200, DB shows `auto_merge=true` |
| `disable_auto_merge` | DELETE same path | 200, DB shows `auto_merge=false` |
| `auto_merge_requires_project_write` | PUT as viewer | 404 |
| `auto_merge_on_closed_mr` | PUT on closed MR | 400 or 404 |

### Step 3.3 — Input validation edge cases (T7)

**File:** `tests/validation_integration.rs` (new file, focused on API boundary validation)

Tests needed (representative subset, one per resource type):
| Test name | Endpoint | Input | Assert |
|---|---|---|---|
| `project_name_null_bytes` | POST `/api/projects` | `{"name": "test\x00evil"}` | 400 |
| `project_name_sql_injection` | POST | `{"name": "'; DROP TABLE projects;--"}` | 400 or 201 (safe — parameterized) |
| `project_name_unicode_emoji` | POST | `{"name": "test-project-🚀"}` | 400 (check_name rejects non-alphanum) |
| `issue_title_xss` | POST `/api/projects/{id}/issues` | `{"title": "<script>alert(1)</script>"}` | 201 (stored safely, no filtering needed) |
| `issue_title_null_bytes` | POST | `{"title": "test\x00"}` | 400 |
| `user_name_special_chars` | POST `/api/users` | `{"name": "admin'; --"}` | 400 |
| `webhook_url_null_bytes` | POST (direct DB insert) | URL with null bytes | 400 |
| `secret_name_null_bytes` | POST `/api/projects/{id}/secrets` | `{"name": "secret\x00"}` | 400 |

**Notes:** `check_name()` allows `[a-zA-Z0-9._-]` — anything outside this should be rejected. Null bytes are the most dangerous edge case (C string termination). SQL injection is safe with parameterized queries but worth testing as defense-in-depth.

### Step 3.4 — Pagination boundary tests (T8)

**File:** `tests/validation_integration.rs` (same new file)

Tests needed:
| Test name | Endpoint | Query | Assert |
|---|---|---|---|
| `pagination_limit_zero` | GET `/api/projects` | `?limit=0` | 200, empty items (or default limit) |
| `pagination_limit_exceeds_max` | GET | `?limit=101` | 200, clamped to 100 items |
| `pagination_negative_limit` | GET | `?limit=-1` | 400 (i64 deserialization may handle) |
| `pagination_negative_offset` | GET | `?offset=-1` | 400 |
| `pagination_offset_beyond_total` | GET | `?offset=999999` | 200, empty items |

**Notes:** Verify actual server behavior first — does axum/serde reject negative i64? Does the handler clamp `limit` to max 100? Test actual behavior, then decide if it needs hardening.

### Step 3.5 — Rate limit boundary tests (T9)

**File:** `tests/auth_integration.rs` (add new tests)

Tests needed:
| Test name | Setup | Assert |
|---|---|---|
| `rate_limit_at_threshold_minus_one` | Pre-set counter to 9 (limit is 10) | Login succeeds |
| `rate_limit_at_threshold` | Pre-set counter to 10 | Login returns 429 |
| `rate_limit_window_expiry` | Pre-set counter to 10 with TTL=1, sleep 1.1s | Login succeeds |

**Notes:** Rate limit uses Valkey INCR with TTL. Test can set the counter directly via `state.valkey`. The `check_rate` function uses prefix `rate:{prefix}:{identifier}`.

### Step 3.6 — Secret decryption path tests (T13, T14)

**File:** `tests/secrets_integration.rs` (add new tests)

Tests needed:
| Test name | Endpoint | Assert |
|---|---|---|
| `read_project_secret_returns_decrypted_value` | GET `/api/projects/{id}/secrets/{name}/value` | 200, value matches what was stored |
| `read_project_secret_not_found` | GET with nonexistent name | 404 |
| `read_project_secret_requires_permission` | GET as unauthorized user | 404 |
| `read_project_secret_audit_logged` | GET | audit_log entry with action=`secret.read` |
| `encryption_round_trip_via_scoped_query` | create secret → `query_scoped_secrets` | decrypted value == original plaintext |

**Notes:** `read_project_secret` uses `Permission::SecretRead` via `require_secret_read`. Route is likely `/api/projects/{id}/secrets/{name}/value`. Test the full round-trip: POST create → GET read → assert value matches.

### Step 3.7 — Token expiry boundary tests (T25)

**File:** `tests/auth_integration.rs` (add new tests)

Tests needed:
| Test name | Setup | Assert |
|---|---|---|
| `create_token_zero_days` | `{"expires_in_days": 0}` | 400 |
| `create_token_negative_days` | `{"expires_in_days": -1}` | 400 |
| `create_token_exceeds_max` | `{"expires_in_days": 366}` | 400 (max is 365) |
| `create_token_at_max` | `{"expires_in_days": 365}` | 200 |
| `create_token_at_min` | `{"expires_in_days": 1}` | 200 |

**Verification after Phase 3:** `just test-integration` targeting modified files.

---

## Phase 4: RBAC & Data Integrity (T15, T16)

### Step 4.1 — Soft-deleted project resource access (T15)

**File:** `tests/project_integration.rs` (add new tests)

Tests needed:
| Test name | Setup | Assert |
|---|---|---|
| `create_issue_on_deleted_project` | Delete project, POST issue | 404 |
| `create_pipeline_on_deleted_project` | Delete project, POST pipeline | 404 |
| `create_webhook_on_deleted_project` | Delete project, POST webhook | 404 |
| `create_secret_on_deleted_project` | Delete project, POST secret | 404 |
| `list_projects_excludes_deleted` | Delete project, GET list | Deleted project not in items |

**Notes:** Soft-delete sets `is_active = false`. All handlers filter `AND is_active = true`. These tests verify that filter is present everywhere.

### Step 4.2 — Workspace-derived permission tests (T16)

**File:** `tests/workspace_integration.rs` (add new tests)

Tests needed:
| Test name | Setup | Assert |
|---|---|---|
| `workspace_member_gets_project_read` | Create workspace → add member → create project in workspace | Member can GET project |
| `workspace_admin_gets_project_write` | Create workspace → add admin → create project | Admin can PATCH project |
| `workspace_non_member_denied` | Non-member user | 404 on project GET (private) |
| `removing_member_removes_access` | Remove member from workspace | Former member gets 404 on project |

**Notes:** `resolver::add_workspace_permissions()` injects implicit permissions. Need to create workspace, add project to it, add member, then test access.

**Verification after Phase 4:** `just test-bin project_integration` and `just test-bin workspace_integration`.

---

## Phase 5: Fix Flaky Tests (T17–T20)

### Step 5.1 — Observe ingest: replace sleep with shutdown-drain (T17)

**File:** `tests/observe_ingest_integration.rs` (4 sites: ~lines 237, 275, 312, 383)

Current pattern:
```rust
let handle = tokio::spawn(flush_spans(pool, rx, shutdown_rx));
sleep(1500ms).await;  // ← remove
shutdown_tx.send(());
handle.await;
```

Fix: Remove the sleep. The flush task already drains on shutdown (final `drain_spans()` before break). Just send shutdown immediately and await the handle:
```rust
let handle = tokio::spawn(flush_spans(pool, rx, shutdown_rx));
drop(spans_tx);  // close channel so try_recv sees Disconnected
let _ = shutdown_tx.send(());  // signal shutdown
let _ = handle.await;  // blocks until drain completes
```

This is deterministic — no timing dependency.

### Step 5.2 — Webhook dispatch: remove unnecessary sleeps (T18)

**File:** `tests/webhook_integration.rs` (4 sites: ~lines 890, 937, 1001, 1091)

For single-delivery tests (lines 890, 937, 1001), wiremock's `verify().await` already polls with backoff. Remove the `sleep(3s)`:
```rust
// Before:
sleep(Duration::from_secs(3)).await;
mock_server.verify().await;
// After:
mock_server.verify().await;  // wiremock handles retry internally
```

For the concurrent test (line 1091), replace `sleep(8s)` with a polling loop:
```rust
let deadline = Instant::now() + Duration::from_secs(15);
loop {
    let reqs = mock_server.received_requests().await.unwrap();
    if reqs.len() >= expected_count { break; }
    if Instant::now() > deadline { panic!("timeout waiting for {} webhooks, got {}", expected_count, reqs.len()); }
    tokio::time::sleep(Duration::from_millis(200)).await;
}
```

### Step 5.3 — Deployment preview cleanup: remove unnecessary sleep (T19)

**File:** `tests/deployment_integration.rs` (~line 896)

`stop_preview_for_branch` is awaited inline in the merge handler. By the time POST `/merge` returns 200, the DB write is complete. The 2s sleep is unnecessary — remove it entirely.

### Step 5.4 — Auth token `last_used_at`: poll instead of sleep (T20)

**File:** `tests/auth_integration.rs` (~line 868)

Replace `sleep(200ms)` with a retry loop:
```rust
let mut found = false;
for _ in 0..20 {
    let row = sqlx::query("SELECT last_used_at FROM api_tokens WHERE token_hash = $1")
        .bind(&token_hash)
        .fetch_one(&state.pool).await.unwrap();
    if row.get::<Option<DateTime<Utc>>, _>("last_used_at").is_some() {
        found = true;
        break;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
}
assert!(found, "last_used_at should be set within 2s");
```

**Verification after Phase 5:** `just test-integration` (full suite — flakiness tests need a real run).

---

## Phase 6: Infrastructure & Remaining HIGH Items (T12, T23, T24)

### Step 6.1 — Webhook payload verification (T12)

**File:** `tests/webhook_integration.rs::webhook_fires_on_issue_create`

After `mock_server.verify().await`, inspect the payload:
```rust
let requests = mock_server.received_requests().await.unwrap();
let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
assert_eq!(body["action"], "created");
assert!(body["issue"]["title"].is_string());
assert!(body["issue"]["number"].is_number());
```

### Step 6.2 — Permission cache TTL in test helpers (T23)

**File:** `tests/helpers/mod.rs::test_state()` and `tests/e2e_helpers/mod.rs::e2e_state()`

After building state, add:
```rust
platform::rbac::resolver::set_cache_ttl(state.config.permission_cache_ttl_secs);
```

This ensures tests exercise the same caching code path as production.

### Step 6.3 — Dashboard known bug tracking (T24)

**File:** `tests/dashboard_integration.rs::dashboard_stats_with_data`

Add explicit assertions codifying the bug so it breaks when fixed:
```rust
// Known bug: dashboard queries 'pipeline_runs' which maps to the pipelines table
// but the query returns 0 because no pipeline_runs view exists yet.
// When fixed, update these assertions to match actual counts.
assert_eq!(body["running_builds"].as_i64().unwrap(), 0, "known bug: pipeline_runs query returns 0");
assert_eq!(body["failed_builds"].as_i64().unwrap(), 0, "known bug: pipeline_runs query returns 0");
```

---

## Execution Order & Verification

| Phase | Effort | New tests | Modified tests | Files touched |
|---|---|---|---|---|
| 1: False positives | Small | 0 | ~25 | 7 existing test files |
| 2: New test files | Large | ~55 | 0 | 5 new test files |
| 3: Security coverage | Large | ~25 | ~10 | 4 existing + 1 new file |
| 4: RBAC/data | Medium | ~9 | 0 | 2 existing files |
| 5: Flaky fixes | Small | 0 | ~10 | 4 existing files |
| 6: Infrastructure | Small | 0 | ~5 | 3 existing files |

**Total:** ~89 new tests, ~50 modified assertions, 6 new files, 13 existing files modified.

### Final verification

After all phases:
```bash
just ci-full   # fmt + lint + deny + test-unit + test-integration + test-e2e + build
```

Read the test report:
```bash
cat test-report-*.txt  # verify all tests pass
```

### What's NOT in scope

- MEDIUM findings (T26–T47) — tier reclassification, mock CLI variants, test naming
- LOW findings (T48–T59) — cosmetic assertion tightening
- New helper functions (e.g., `post_multipart`) — build inline in the tests that need them, extract to helper later if reused
