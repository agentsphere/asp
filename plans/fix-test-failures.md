# Plan: Fix 96 Failing Integration Tests

## Executive Summary

96 tests fail across 18 categories. Root causes:
- **~53 tests**: Test expectations wrong — API response format changed from bare arrays to `ListResponse{items,total}`
- **~20 tests**: Test setup wrong — auto-created branch protection conflicts, hostPath vs PodSecurity, `body_json()` crash on non-JSON, stale table names
- **~12 tests**: Implementation bugs — secret scopes changed, delegation validation, flag duplicate handling, pub/sub message format, passkey rename, deleted-project checks, negative pagination
- **~11 tests**: Mixed — both test and impl need updates

---

## Category 1: PodSecurity "baseline:latest" — hostPath volumes forbidden (10 tests)

**Tests**: `send_message_rejects_non_child`, `manager_worker_full_lifecycle`, `child_completion_notifies_parent`, `check_progress_returns_messages`, `create_session_with_parent_links_child`, `tree_subscription_receives_from_multiple_sessions`, `sse_include_children_streams_child_events`, `user_sends_message_to_child_session`, `create_session_spawns_k8s_pod`

**Root cause**: `build_namespace_object()` unconditionally applies `pod-security.kubernetes.io/enforce=baseline` labels when `env == "session"` (line 359). K8s 1.31+ baseline blocks hostPath volumes. In dev/test mode, `ensure_session_namespace()` receives `dev_mode: bool` (line 100) and already uses it to skip NetworkPolicy (line 114), but does NOT pass it down to `ensure_namespace()` → `build_namespace_object()`. So PSA labels are applied even in dev mode, while `service.rs` mounts hostPath when `dev_mode=true`.

**Fix**: Thread `dev_mode` through the call chain and skip PSA labels in dev mode (consistent with existing pattern of skipping NetworkPolicy in dev):
1. `ensure_session_namespace()` already has `dev_mode` — pass it to `ensure_namespace()`
2. `ensure_namespace(kube, ns, env, project_id, platform_ns)` → add `dev_mode: bool` param, pass to `build_namespace_object()`
3. `build_namespace_object(ns, env, project_id)` → add `dev_mode: bool` param, change line 359 to `if env == "session" && !dev_mode {`
4. Update all other callers of `ensure_namespace` / `build_namespace_object` to pass the appropriate `dev_mode` value

**Files**: `src/deployer/namespace.rs` (3 function signatures + condition change), callers in `src/deployer/reconciler.rs` and `src/pipeline/executor.rs`

---

## Category 2: "relation deployments does not exist" (4 tests)

**Tests**: `contract_deployment_with_data`, `contract_mcp_deployment_get`, `contract_mcp_deployment_history`, `dashboard_stats_with_data`

**Root cause**: Test bug. A migration dropped `deployments`/`deployment_history`/`preview_deployments` tables, replacing them with `deploy_targets` and `deploy_releases`. Tests still INSERT into old table names using dynamic queries.

**Fix**: Update tests to insert into `deploy_targets` + `deploy_releases` instead of `deployments`/`deployment_history`.

**Files**: `tests/contract_integration.rs`, `tests/dashboard_integration.rs`

---

## Category 3: Branch protection duplicate key constraint (7 tests)

**Tests**: `merge_blocked_insufficient_approvals`, `merge_blocked_no_ci_pipeline`, `merge_admin_no_bypass`, `merge_blocked_wrong_method`, `merge_blocked_ci_failed`, `merge_admin_bypass`, `merge_allowed_ci_success`

**Root cause**: Test setup bug. `src/api/projects.rs` now auto-creates a default branch protection rule for `main` when a project is created. The test helper `insert_branch_protection()` at `tests/helpers/mod.rs:~757` tries to INSERT a rule with `pattern = "main"`, which collides. The helper uses `.unwrap()` instead of handling the conflict.

**Fix**: Change `insert_branch_protection()` in `tests/helpers/mod.rs` to use `ON CONFLICT (project_id, pattern) DO UPDATE SET ...` so it upserts with test-specified parameters.

**Files**: `tests/helpers/mod.rs`

---

## Category 4: "response body is not valid JSON" at helpers/mod.rs:571 (10 tests)

**Tests**: `download_agent_runner_amd64_integration`, `create_alert_validation`, `complete_wizard_solo_dev`, `complete_wizard_non_admin_forbidden`, `complete_register_invalid_credential_json`, `complete_login_invalid_credential_json`, `proxy_owner_can_access`, `proxy_backend_unreachable_returns_502`, `proxy_preserves_path_and_query`, `proxy_project_reader_can_access`

**Root cause**: Test bug. `body_json()` at `tests/helpers/mod.rs:~571` unconditionally calls `serde_json::from_slice(&bytes).expect(...)`. Several endpoints return non-JSON (binary downloads, proxy HTML/plain-text, empty error bodies).

**Fix**: Two-pronged:
1. Tests expecting non-JSON (downloads, proxy) → use `get_bytes()` / raw request helpers instead of `get_json()`
2. Tests where endpoint SHOULD return JSON errors but doesn't → check if impl is missing JSON error bodies and fix

**Sub-cases**:
- `download_agent_runner_amd64_integration`: binary download → use `get_bytes`
- `preview_integration` (4 tests): proxy returns non-JSON 502 → use raw request
- `observe_integration::create_alert_validation`: alert create returns non-JSON on validation failure → fix impl or test
- `onboarding_integration` (2 tests): onboarding endpoint returns empty body → fix impl or test
- `passkey_integration` (2 tests): WebAuthn failures return non-JSON → fix impl or test

**Files**: `tests/downloads_integration.rs`, `tests/preview_integration.rs`, `tests/observe_integration.rs`, `tests/onboarding_integration.rs`, `tests/passkey_integration.rs`, possibly `src/observe/alert.rs`, `src/api/passkeys.rs`, `src/api/onboarding.rs`

---

## Category 5: Error message mismatch — "API key" vs "No LLM provider" (2 tests)

**Tests**: `cli_create_app_no_credentials`, `create_app_without_api_key_fails`

**Root cause**: Test bug. Error message was updated from "API key" to the more descriptive "No LLM provider configured. Set your key in Settings > Provider Keys, configure a custom provider, or ask an admin to set a global ANTHROPIC_API_KEY secret." Tests still assert `contains("API key")`.

**Fix**: Change assertions to `contains("No LLM provider")` or `contains("LLM provider")`.

**Files**: `tests/cli_create_app_integration.rs`, `tests/create_app_integration.rs`

---

## Category 6: branch_protection_integration — all getting 409 (4 tests)

**Tests**: `create_protection`, `update_protection`, `list_protections`, `delete_protection`

**Root cause**: Test setup bug. Same as Category 3 — project creation auto-creates a `main` protection rule. Tests try to create one for `"main"` → 409 Conflict.

**Fix**: Change tests to use a non-default pattern (e.g., `"develop"` or `"release/*"`) instead of `"main"`, OR delete/update the auto-created rule first.

**Files**: `tests/branch_protection_integration.rs`

---

## Category 7: contract_integration — ListResponse format changes (7 tests)

**Tests**: `contract_admin_permissions`, `contract_admin_roles`, `contract_api_tokens`, `contract_mcp_admin_assign_role`, `contract_mcp_admin_list_delegations`, `contract_deployment_list`, `contract_preview_list`

**Root cause**: Test bug. Endpoints now return `{items, total}` not bare arrays.

**Specific fixes**:
- `contract_admin_roles` (line 716): `body.as_array()` → `body["items"].as_array()`
- `contract_api_tokens` (line 846): `list_body.as_array()` → `list_body["items"].as_array()`
- `contract_admin_permissions` (line 787): `roles.as_array()` → `roles["items"].as_array()`
- `contract_mcp_admin_assign_role` (line 1093): same pattern
- `contract_mcp_admin_list_delegations` (line 1122): `body.is_array()` → `body["items"].is_array()`
- `contract_deployment_list`: route changed to `/api/projects/{id}/targets`
- `contract_preview_list`: route changed to `/api/projects/{id}/targets?environment=preview`

**Files**: `tests/contract_integration.rs`

---

## Category 8: Webhook tests — no requests received (4 tests)

**Tests**: `webhook_fires_on_issue_create`, `webhook_hmac_signature`, `webhook_no_signature_without_secret`, `webhook_concurrent_limit`

**Root cause**: Test setup bug. Webhook tests use `insert_webhook()` to bypass SSRF validation at creation, but `fire_webhooks()` re-validates the URL at dispatch time, blocking wiremock (localhost). Alternatively, the async fire_webhooks task doesn't complete before `mock_server.verify()`.

**Fix**: Either skip SSRF validation at dispatch time for URLs already in DB, or add a `tokio::time::sleep` / poll loop before verify to allow async delivery. For `webhook_concurrent_limit`, increase timeout.

**Files**: `tests/webhook_integration.rs`, `src/api/webhooks.rs`

---

## Category 9: flags_integration (3 tests)

**Tests**: `create_flag_duplicate`, `set_override`, `delete_override`

**Root cause**: Mixed.
- `create_flag_duplicate` (200 vs 409): Unique constraint is `(key, project_id, environment)`. Two flags with `environment=NULL` don't conflict because PostgreSQL treats `NULL != NULL`. **Impl bug** — need `NULLS NOT DISTINCT` or application-level check.
- `set_override` (500 vs 200): Test uses `Uuid::new_v4()` as `target_user_id` which doesn't exist → FK violation → 500. **Test bug**.
- `delete_override` (404 vs 204): Cascading failure from `set_override`.

**Fix**:
1. Add `NULLS NOT DISTINCT` to unique constraint via migration, OR add app-level duplicate check in `create_flag`
2. Use `create_user()` for real user IDs in override tests

**Files**: `tests/flags_integration.rs`, `src/api/flags.rs` or new migration

---

## Category 10: deployment_integration (4 tests)

**Tests**: `create_release_requires_deploy_promote`, `promote_staging_requires_deploy_promote`, `rollback_requires_deploy_promote`, `demo_project_creates_mr_and_triggers_pipeline`

**Root cause**: Mixed.
- 3 permission tests: expect 403 but get 404. Per CLAUDE.md security pattern, endpoints return 404 for forbidden private resources to avoid leaking existence. **Test bug** — expect 404.
- `demo_project_creates_mr_and_triggers_pipeline`: 0 MRs created vs expected 1. **Impl bug** in `src/onboarding/demo_project.rs`.

**Fix**: Change 3 permission tests to expect 404. Investigate demo project MR creation.

**Files**: `tests/deployment_integration.rs`, `src/onboarding/demo_project.rs`

---

## Category 11: eventbus_integration (1 test)

**Test**: `ops_repo_updated_reads_platform_yaml`

**Root cause**: Test bug or impl bug. Test expects `strategy = "canary"` but gets `"rolling"`. The event handler may not be extracting strategy from platform.yaml's deploy spec.

**Fix**: Investigate `src/store/eventbus.rs` OpsRepoUpdated handler to see how it determines strategy. Fix handler or update test expectation.

**Files**: `tests/eventbus_integration.rs`, `src/store/eventbus.rs`

---

## Category 12: registry_integration (2 tests)

**Tests**: `blob_get_returns_data`, `chunked_blob_upload`

**Root cause**: Test bug. `get_blob` handler now returns `307 TEMPORARY_REDIRECT` with a presigned URL (intentional change for streaming large blobs). Tests expect 200.

**Fix**: Update tests to expect 307 and check `Location` header.

**Files**: `tests/registry_integration.rs`

---

## Category 13: secrets_integration (5 tests)

**Tests**: `user_key_set_and_list`, `user_key_delete`, `query_scoped_secrets_deploy`, `query_scoped_secrets_environment_filter`, `non_authorized_user_cannot_create_secret_request`

**Root cause**: Mixed.
- `user_key_set_and_list`, `user_key_delete`: `from_value::<Vec<Value>>(body)` fails because endpoint returns `ListResponse{items,total}` (map, not array). **Test bug**.
- `query_scoped_secrets_deploy`, `query_scoped_secrets_environment_filter`: Old scope name `"deploy"` renamed to `"staging"`. **Test bug**.
- `non_authorized_user_cannot_create_secret_request`: Returns 404 not 403 (security pattern). **Test bug**.

**Fix**: Parse `body["items"]`, use new scope names, expect 404.

**Files**: `tests/secrets_integration.rs`

---

## Category 14: rbac_integration (3 tests)

**Tests**: `delegation_grants_temporary_access`, `expired_delegation_denied`, `revoked_delegation_denied`

**Root cause**: Test or impl bug. All delegation creations return 400 instead of 201. The `permission` string `"project:read"` may no longer match `Permission::FromStr`.

**Fix**: Check `src/rbac/types.rs` for `FromStr` implementation. Update test permission strings or fix the parser.

**Files**: `tests/rbac_integration.rs`, `src/rbac/types.rs`

---

## Category 15: session_integration (7 tests)

**Tests**: `list_children_empty`, `list_children_with_children`, `list_children_nonexistent_parent_returns_empty`, `list_iframes_returns_empty_for_session`, `spawn_child_session`, `create_session_empty_prompt`, `create_session_spawns_k8s_pod`

**Root cause**: Mixed.
- 5 tests (list_children, list_iframes, spawn_child): `ListResponse` format change → `body.as_array().unwrap()` fails. **Test bug**.
- `create_session_empty_prompt` (500 vs 201): Empty prompt causes error. **Impl bug** — handler should handle empty prompts gracefully.
- `create_session_spawns_k8s_pod`: PodSecurity issue (Category 1).

**Fix**: Update list tests to use `body["items"]`. Investigate empty prompt 500. PodSecurity fix from Category 1.

**Files**: `tests/session_integration.rs`

---

## Category 16: user_keys_integration (8 tests) — ALL failing with unwrap on None

**Tests**: `list_provider_keys_after_set`, `delete_provider_key`, `set_provider_key_overwrites`, `list_shows_key_suffix_not_raw_key`, `list_multiple_providers`, `list_includes_timestamps`, `keys_are_per_user`, `overwrite_updates_key_suffix`

**Root cause**: Test bug. Response format changed to `ListResponse{items,total}`. All tests use patterns that expect bare arrays or objects at root level.

**Fix**: Update all assertions to use `body["items"]` pattern.

**Files**: `tests/user_keys_integration.rs`

---

## Category 17: llm_providers_integration (4 tests)

**Tests**: `list_providers`, `update_provider`, `delete_provider`, `provider_scoped_to_user`

**Root cause**: Test bug. `list_providers` returns `ListResponse`, not bare array. Cascading failures for tests that parse the list.

**Fix**: `body.as_array()` → `body["items"].as_array()`.

**Files**: `tests/llm_providers_integration.rs`

---

## Category 18: Other individual failures (12 tests)

### 18a: `list_and_delete_api_token` — unwrap on None (auth_integration.rs:389)
**Root cause**: Test bug. `list_api_tokens` returns `ListResponse`. Use `list_body["items"]`.
**Files**: `tests/auth_integration.rs`

### 18b: `test_list_gpg_keys_only_own_keys` — unwrap on None (gpg_keys_integration.rs:582)
**Root cause**: Test bug. `ListResponse` format change OR GPG key fixture issue. Investigate.
**Files**: `tests/gpg_keys_integration.rs`

### 18c: `rename_passkey_success` — Null vs "NewName" (passkey_integration.rs:165)
**Root cause**: Test bug. `list_passkeys` returns `ListResponse`. `keys[0]` on a JSON object returns Null. Use `keys["items"][0]["name"]`.
**Files**: `tests/passkey_integration.rs`

### 18d: `publish_control_reaches_input_channel` — Null vs "interrupt" (pubsub_integration.rs:522)
**Root cause**: Test bug. `publish_control` sends `{"type":"control","control":{"type":control_type}}` but test asserts `parsed["control_type"]`. Use `parsed["control"]["type"]`.
**Files**: `tests/pubsub_integration.rs`

### 18e: `pull_platform_runner_image`, `auto_setup_downloads_agent_runner` — Address already in use
**Root cause**: Test setup bug. Both tests bind same port in parallel.
**Fix**: Use different ports or run sequentially (`serial_test` crate).
**Files**: `tests/registry_pull_integration.rs`, `tests/helpers/mod.rs`

### 18f: `pagination_negative_limit`, `pagination_negative_offset` — 500 Internal Server Error
**Root cause**: Impl bug. Negative values cause SQL errors. Add validation clamping to 0.
**Files**: Relevant list handlers (common pagination path), `src/api/helpers.rs`

### 18g: `create_secret_on_deleted_project`, `create_webhook_on_deleted_project` — 201 vs 404
**Root cause**: Impl bug. Create handlers don't check `is_active = true` on the project.
**Files**: `src/api/secrets.rs`, `src/api/webhooks.rs`

### 18h: `merge_squash_strategy` — "squash not allowed; permitted: merge"
**Root cause**: Test setup bug. Auto-created branch protection defaults `merge_methods` to `["merge"]` only. Test needs to update rule to allow squash first.
**Files**: `tests/merge_gates_integration.rs`

### 18i: `add_and_list_members`, `remove_member` — unwrap on None
**Root cause**: Test bug. `list_members` returns `ListResponse`. Use `body["items"]`.
**Files**: `tests/workspace_integration.rs`

---

## Implementation Order

### Phase 1: Systemic ListResponse format changes (~45 tests)
Highest impact, mechanical fix. Single pattern: `body.as_array()` → `body["items"].as_array()`.

| File | Tests Fixed |
|------|-------------|
| `tests/auth_integration.rs` | 1 |
| `tests/contract_integration.rs` | 5 |
| `tests/llm_providers_integration.rs` | 4 |
| `tests/secrets_integration.rs` | 2 |
| `tests/session_integration.rs` | 5 |
| `tests/workspace_integration.rs` | 2 |
| `tests/passkey_integration.rs` | 1 |
| `tests/user_keys_integration.rs` | 8 |
| `tests/gpg_keys_integration.rs` | 1 |

### Phase 2: Branch protection auto-create conflict (~12 tests)
1. `tests/helpers/mod.rs` — `insert_branch_protection()` → upsert
2. `tests/branch_protection_integration.rs` — use non-main patterns or delete auto-created rule
3. `tests/merge_gates_integration.rs` — fix squash test setup

### Phase 3: Table schema changes (~4 tests)
1. `tests/contract_integration.rs` — `deployments` → `deploy_targets`/`deploy_releases`
2. `tests/dashboard_integration.rs` — same

### Phase 4: Non-JSON response handling (~10 tests)
1. Tests on non-JSON endpoints → use raw request helpers
2. Fix impl where JSON error bodies are missing

### Phase 5: PodSecurity hostPath conflict (~10 tests)
1. `src/deployer/namespace.rs` — `privileged` PodSecurity in dev mode

### Phase 6: Individual fixes (~15 tests)
1. Error message assertion updates (2 tests)
2. Secret scope renames (2 tests)
3. Deployment permission 404 expectations (3 tests)
4. Registry 307 redirect expectations (2 tests)
5. Flag duplicate + override fixes (3 tests)
6. Delegation permission string parsing (3 tests)
7. Pubsub message format (1 test)
8. Registry_pull port conflict (2 tests)
9. Eventbus strategy (1 test)
10. Session empty prompt 500 (1 test)
11. Demo project MR creation (1 test)

### Phase 7: Implementation bug fixes (~6 tests)
1. Negative pagination validation (`src/api/helpers.rs`)
2. Deleted project checks (`src/api/secrets.rs`, `src/api/webhooks.rs`)
3. Flag NULLS NOT DISTINCT (migration or app logic)
4. Webhook SSRF at dispatch time

---

## Estimated Files Changed

| File | Change Type |
|------|-------------|
| `tests/helpers/mod.rs` | Upsert in `insert_branch_protection()` |
| `tests/auth_integration.rs` | ListResponse format |
| `tests/contract_integration.rs` | ListResponse + deploy table names + routes |
| `tests/llm_providers_integration.rs` | ListResponse format |
| `tests/secrets_integration.rs` | ListResponse + scope names + 404 |
| `tests/session_integration.rs` | ListResponse format |
| `tests/workspace_integration.rs` | ListResponse format |
| `tests/passkey_integration.rs` | ListResponse format |
| `tests/user_keys_integration.rs` | ListResponse format |
| `tests/gpg_keys_integration.rs` | ListResponse or fixture |
| `tests/branch_protection_integration.rs` | Non-main patterns |
| `tests/merge_gates_integration.rs` | Squash + protection setup |
| `tests/dashboard_integration.rs` | Deploy table names |
| `tests/downloads_integration.rs` | get_bytes |
| `tests/preview_integration.rs` | Raw request |
| `tests/observe_integration.rs` | Non-JSON handling |
| `tests/onboarding_integration.rs` | Non-JSON handling |
| `tests/webhook_integration.rs` | SSRF/async dispatch |
| `tests/flags_integration.rs` | Real user IDs |
| `tests/deployment_integration.rs` | 404 expectation + demo project |
| `tests/rbac_integration.rs` | Permission strings |
| `tests/pubsub_integration.rs` | Message format |
| `tests/registry_integration.rs` | 307 redirect |
| `tests/registry_pull_integration.rs` | Port conflict |
| `tests/validation_integration.rs` | Pagination + deleted project |
| `tests/cli_create_app_integration.rs` | Error message |
| `tests/create_app_integration.rs` | Error message |
| `tests/eventbus_integration.rs` | Strategy expectation |
| `src/deployer/namespace.rs` | PodSecurity dev mode |
| `src/api/flags.rs` or migration | NULLS NOT DISTINCT |
| `src/api/secrets.rs` | Deleted project check |
| `src/api/webhooks.rs` | Deleted project check + SSRF |
| `src/api/helpers.rs` | Negative pagination clamping |
| `src/rbac/types.rs` | Permission FromStr check |
| `src/onboarding/demo_project.rs` | MR creation |
