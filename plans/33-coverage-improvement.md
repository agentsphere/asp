# Plan 33 — Test Coverage Improvement

## Context

Current combined line coverage is **88.61%** (15,695 lines, 1,788 missed) across unit + integration + E2E. The in-process agent mock (Plan 32 follow-up) recovered ~200 lines. This plan targets the remaining high-value gaps to push coverage past **93%**.

**Baseline after inprocess mock:** ~614 integration tests, 767 unit tests, 49 E2E tests.

---

## Coverage Gaps by Priority

### Tier 1 — High value, straightforward (~450 missed lines → ~93%)

| # | File | Coverage | Missed | Target |
|---|---|---|---|---|
| 1 | `api/sessions.rs` | 72% | 129 | 90%+ |
| 2 | `store/eventbus.rs` | 67% | 84 | 85%+ |
| 3 | `deployer/reconciler.rs` | 79% | 72 | 90%+ |
| 4 | `observe/ingest.rs` | 89% | 50 | 95%+ |
| 5 | `observe/query.rs` | 84% | 45 | 92%+ |
| 6 | `deployer/ops_repo.rs` | 87% | 43 | 93%+ |
| 7 | `api/webhooks.rs` | 75% | 48 | 88%+ |

### Tier 2 — Moderate value, harder (~500 missed lines)

| # | File | Coverage | Missed | Target |
|---|---|---|---|---|
| 8 | `pipeline/executor.rs` | 85% | 153 | 88%+ |
| 9 | `agent/service.rs` | 50% | 102 | 75%+ |
| 10 | `api/passkeys.rs` | 53% | 67 | 75%+ |

---

## PR 1: `api/sessions.rs` — Session Handler Coverage (+129 lines)

**File:** `tests/session_integration.rs` (extend existing)

### New tests

```
session_send_message_to_nonexistent_session          → 404 response
session_send_message_empty_content_rejected           → 400 validation
session_stop_already_stopped_session                  → 400 or idempotent 200
session_create_app_scoped_token_forbidden             → 403 (scope_project_id != None)
session_create_app_without_agent_run_permission       → 403
session_create_app_description_too_long               → 400 validation
session_update_session_bind_to_project                → PATCH binds session.project_id
session_update_session_already_bound                  → 409 or error
session_spawn_child_depth_limit_enforced              → depth=5 → 400 "max depth"
session_spawn_child_parent_not_running                → 400 "not running"
session_validate_provider_config_invalid_role          → 400
session_validate_provider_config_browser_non_ui_role   → 400
session_list_children_empty                            → empty items array
session_list_children_nonexistent_parent               → 404
```

**Expected line recovery:** ~100 of 129 missed lines

---

## PR 2: `store/eventbus.rs` — Event Bus Integration Tests (+84 lines)

**File:** `tests/eventbus_integration.rs` (new)

The eventbus subscribes to Valkey pub/sub and triggers deployment actions. Tests need a real Valkey + DB.

### New tests

```
eventbus_image_built_creates_default_deployment       → first build triggers deployment row
eventbus_image_built_updates_existing_deployment      → subsequent build updates image_ref
eventbus_image_built_no_ops_repo_still_deploys        → deployment without ops repo works
eventbus_ops_repo_updated_triggers_reconcile          → ops repo change wakes deployer
eventbus_rollback_requested_reverts_ops_repo          → rollback event reverts last commit
eventbus_rollback_no_history_returns_error            → rollback with no prior deployment
eventbus_malformed_event_ignored                      → invalid JSON in channel is skipped
eventbus_deploy_requested_triggers_deployment         → manual deploy request works
```

### Approach

1. Publish events directly to Valkey channel `platform:events`
2. Start eventbus subscriber in test with `tokio::spawn`
3. Wait for DB state changes (poll with timeout)
4. Verify deployment rows, ops repo commits, reconciler notifications

**Expected line recovery:** ~60 of 84 missed lines

---

## PR 3: `deployer/reconciler.rs` — Reconciler Edge Cases (+72 lines)

**File:** `tests/e2e_deployer.rs` (extend existing)

### New tests

```
reconciler_no_pending_deployments_is_noop             → reconcile with empty queue
reconciler_concurrent_claim_loses_race                → optimistic lock conflict
reconciler_apply_failure_marks_failed                 → K8s apply error → status=failed
reconciler_rollback_no_ops_repo_uses_db_history       → legacy DB-based rollback path
reconciler_rollback_ops_repo_revert_failure           → git revert conflict → status=failed
reconciler_stopped_scales_to_zero                     → scale down path exercised
reconciler_basic_manifest_generation                  → no ops repo → generates basic Deployment
reconciler_finalize_success_writes_history            → deployment_history row created
```

### Approach

- Insert deployment rows directly in DB with various `desired_status` values
- Spawn reconciler, let it process one cycle
- Assert final `status`, `deployment_history` rows, K8s resources

**Expected line recovery:** ~50 of 72 missed lines

---

## PR 4: `observe/ingest.rs` + `observe/query.rs` — Observability Edge Cases (+95 lines)

### `observe/ingest.rs` — Unit tests in `src/observe/ingest.rs`

```
build_log_record_missing_trace_id                     → log without trace context
build_log_record_zero_timestamp_uses_now              → fallback to current time
build_log_record_empty_severity                       → defaults to "UNSPECIFIED"
build_metric_records_sum_type                         → Sum metric parsing
build_metric_records_histogram_type                   → Histogram metric parsing
build_metric_records_no_data_points                   → empty metric → empty vec
build_span_record_unfinished_span                     → end_time=0 handling
build_span_record_overflow_duration                   → >i32::MAX nanoseconds clamped
```

### `observe/query.rs` — Integration tests in `tests/observe_integration.rs`

```
query_logs_all_filters_combined                       → project + level + service + q + time
query_logs_empty_result                               → no matching logs → empty items
query_traces_invalid_status_filter                    → 400 or ignored
query_metrics_missing_name                            → 400 "name required"
query_metrics_invalid_labels_json                     → 400 parse error
query_metrics_empty_result                            → no data → empty array
query_metric_names_empty                              → no metrics → empty array
query_session_timeline_no_events                      → empty timeline
```

**Expected line recovery:** ~70 of 95 missed lines

---

## PR 5: `deployer/ops_repo.rs` — Ops Repo Error Paths (+43 lines)

### Unit tests in `src/deployer/ops_repo.rs` (extend `#[cfg(test)]`)

```
init_ops_repo_already_exists                          → idempotent or error
read_file_at_ref_nonexistent_file                     → error returned
read_file_at_ref_nonexistent_ref                      → error returned
read_values_invalid_yaml                              → parse error
commit_values_no_changes                              → "nothing to commit" handled
revert_last_commit_initial_commit                     → error (nothing to revert)
resolve_manifest_path_traversal_blocked               → ".." rejected
cleanup_worktree_nonexistent                          → no error (idempotent)
```

**Expected line recovery:** ~30 of 43 missed lines

---

## PR 6: `api/webhooks.rs` — Webhook Delivery & SSRF (+48 lines)

**File:** `tests/webhook_integration.rs` (extend existing)

### New tests

```
webhook_ssrf_ftp_scheme_rejected                      → ftp:// → 400
webhook_ssrf_file_scheme_rejected                     → file:// → 400
webhook_ssrf_cloud_metadata_rejected                  → 169.254.169.254 → 400
webhook_update_events_only                            → PATCH with only events field
webhook_update_url_only                               → PATCH with only url field
webhook_update_deactivate_and_reactivate              → toggle is_active
webhook_delete_nonexistent                            → 404
webhook_test_endpoint_fires_test_event                → test payload delivered (mock server)
```

### Webhook delivery test approach

The SSRF filter blocks localhost URLs. To test actual delivery:
1. Use a non-loopback IP that routes locally (e.g., bind mock server to `0.0.0.0`, use machine's real LAN IP)
2. Or insert webhook row with URL directly in DB (bypassing SSRF check) and test `fire_webhooks()` function directly
3. Verify HMAC-SHA256 signature on received request

**Expected line recovery:** ~35 of 48 missed lines

---

## PR 7: `agent/service.rs` — Service Layer Coverage (+102 lines)

**File:** `tests/agent_service_integration.rs` (new)

### New tests

```
service_get_provider_unknown_returns_error            → "unknown" → InvalidProvider
service_get_provider_claude_code_succeeds             → "claude-code" → Ok
service_create_global_session_no_key_fails            → missing API key → error
service_fetch_session_nonexistent                     → 404-equivalent error
service_stop_session_already_stopped                  → idempotent or error
service_resolve_user_api_key_no_master_key            → None when MASTER_KEY unset
service_resolve_user_api_key_no_user_key              → None when user has no key
```

These use the mock Anthropic server from `tests/mock_anthropic.rs` for session creation tests.

**Expected line recovery:** ~50 of 102 missed lines

---

## PR 8: `api/passkeys.rs` — WebAuthn Edge Cases (+67 lines)

**File:** `tests/passkey_integration.rs` (extend existing)

### New tests

```
passkey_register_agent_user_rejected                  → user_type=agent → 403
passkey_rename_nonexistent                            → 404
passkey_rename_wrong_user                             → 404 (not 403)
passkey_delete_nonexistent                            → 404
passkey_delete_wrong_user                             → 404
passkey_login_no_passkeys_registered                  → begin_login error
passkey_complete_login_invalid_credential             → 401
```

### WebAuthn mock approach

The `webauthn-rs` crate provides `SoftwarePasskey` for testing:
```rust
use webauthn_rs::prelude::SoftwarePasskey;
let mut key = SoftwarePasskey::new();
// Use key.do_registration() and key.do_authentication() for full ceremony
```

**Expected line recovery:** ~40 of 67 missed lines

---

## Implementation Order

| PR | Focus | New Tests | Lines Recovered | Effort |
|---|---|---|---|---|
| 1 | sessions.rs | ~14 | ~100 | 1 day |
| 2 | eventbus.rs | ~8 | ~60 | 1 day |
| 3 | reconciler.rs | ~8 | ~50 | 1 day |
| 4 | observe (ingest + query) | ~16 | ~70 | 1 day |
| 5 | ops_repo.rs | ~8 | ~30 | 0.5 day |
| 6 | webhooks.rs | ~8 | ~35 | 0.5 day |
| 7 | agent/service.rs | ~7 | ~50 | 0.5 day |
| 8 | passkeys.rs | ~7 | ~40 | 0.5 day |
| **Total** | | **~76** | **~435** | **~6 days** |

---

## Coverage Projection

| Metric | Before | After |
|---|---|---|
| Total lines | 15,695 | 15,695 |
| Missed lines | 1,788 | ~1,350 |
| Line coverage | 88.61% | **~91.4%** |
| Unit tests | 767 | ~790 |
| Integration tests | 614 | ~685 |

With Tier 2 work (PRs 7-8), total could reach **~93%**.

---

## Verification

After each PR:
1. `just test-unit` — all unit tests pass
2. `just test-integration` — all integration tests pass
3. `cargo llvm-cov report --ignore-filename-regex '(proto\.rs|ui\.rs|main\.rs)'` — check per-file coverage
4. After all PRs: `just cov-total` — combined coverage ≥ 91%

---

## Files Summary

### New test files
- `tests/eventbus_integration.rs` (~250 lines)
- `tests/agent_service_integration.rs` (~200 lines)

### Extended test files
- `tests/session_integration.rs` (+~400 lines)
- `tests/observe_integration.rs` (+~200 lines)
- `tests/webhook_integration.rs` (+~200 lines)
- `tests/passkey_integration.rs` (+~200 lines)
- `tests/e2e_deployer.rs` (+~250 lines)
- `src/observe/ingest.rs` (+~100 lines, unit tests in `#[cfg(test)]`)
- `src/deployer/ops_repo.rs` (+~100 lines, unit tests in `#[cfg(test)]`)

### Source changes
- None required. All tests use existing public APIs and test helpers.
- Exception: `tests/mock_anthropic.rs` already created (Plan 32 follow-up).
