# Test Quality Audit — 2026-03-25

**Suite:** 1,385 unit tests · 937 integration tests · 9 E2E files · 38 integration files
**Health:** Strong foundation with systematic gaps in coverage, assertion quality, and edge cases.

---

## Inventory

| Module | Unit tests | Integration file | E2E file | Key gaps |
|---|---|---|---|---|
| api/releases | 0 | — | — | **ZERO tests** |
| api/branch_protection | 0 | — | — | **ZERO tests** |
| api/llm_providers | 0 | — | — | **ZERO tests** |
| api/onboarding | 0 | — | — | **ZERO tests** |
| api/flags | 0 | deployment_integration (eval only) | — | CRUD untested |
| api/merge_requests | — | issue_mr_integration | e2e_demo | merge gates untested |
| api/secrets | ✓ | secrets_integration | — | decrypt read untested |
| api/deployments | — | deployment_integration | e2e_deployer | ops repo CRUD gaps |
| api/sessions | ✓ | session_integration | e2e_agent | SSE untested |
| auth | ✓ | auth_integration | — | rate limit boundary gaps |
| rbac | ✓ | rbac_integration | — | workspace perms untested |
| rbac/delegation | — | rbac_integration, admin | — | no unit tests |
| pipeline | ✓ | pipeline_integration | e2e_pipeline | — |
| deployer/ops_repo | ✓ (I/O!) | deployment_integration | e2e_deployer | — |
| agent | ✓ | session_integration | e2e_agent | — |
| observe | ✓ | observe_integration | — | value integrity weak |
| git | ✓ (I/O!) | git_smart_http, git_browse | e2e_git, e2e_ssh | LFS gaps |
| notify | — | notification_integration | — | dispatch unit tests missing |
| registry | ✓ | registry_integration | — | edge cases missing |
| secrets/engine | ✓ | secrets_integration | — | round-trip via API untested |

---

## Findings

### CRITICAL — Fix immediately

**T1: Five API modules with ZERO integration tests**
- `src/api/releases.rs` — 7 handlers (CRUD + asset upload/download)
- `src/api/branch_protection.rs` — 5 handlers (security-critical: gates merges)
- `src/api/llm_providers.rs` — 7 handlers (encrypted API key storage)
- `src/api/onboarding.rs` — 10+ handlers (wizard, OAuth flow, presets)
- `src/api/flags.rs` — 10+ CRUD handlers (only evaluation tested)

**T2: `enforce_merge_gates()` has zero test coverage** (`src/api/merge_requests.rs:499-604`)
- Checks required approvals, CI status, merge method restrictions, admin bypass
- Primary security enforcement for code review and CI gates
- No integration test exercises any gate condition

**T3: `auto_merge` enable/disable endpoints untested** (`src/api/merge_requests.rs:1325-1418`)
- HTTP endpoints never called in any test
- Only tested indirectly via e2e_demo demo project creation

**T4: False positive — `alert_fired_sets_cooldown_on_attempt`** (`tests/eventbus_integration.rs`)
- Only asserts `result.is_ok()` — passes whether or not cooldown is set
- Compare with `alert_fired_warning_severity_proceeds` which correctly checks Valkey

**T5: False positive — `setup_creates_personal_workspace`** (`tests/setup_integration.rs`)
- POST response status assigned to `_status` (discarded)
- Test passes even if endpoint returns 500

**T6: Security invariant violation — OR-assertion on status code** (`tests/pipeline_integration.rs::private_project_returns_404_for_non_member`)
- Accepts `404 || 403` — CLAUDE.md mandates 404 only for private resources
- Every other permission test correctly asserts `== NOT_FOUND`

**T7: No Unicode/null byte/special character tests** (all integration files)
- Zero tests for null bytes, SQL injection payloads, `<script>` tags in text fields
- `check_name()` validator exists but API-level rejection never tested

**T8: No pagination boundary value tests** (all list endpoints)
- Never tests `limit=0`, `limit=101` (exceeds max), `limit=-1`, `offset=-1`
- Current tests only use `limit=2` or `limit=5`

**T9: Rate limit boundary tests missing** (`tests/auth_integration.rs`)
- No test for exact threshold (count=9 succeeds, count=10 fails)
- No test for window expiry
- Only login is tested; other endpoints' rate limits unknown

---

### HIGH — Fix before release

**T10: 14 tests discard response body on mutation endpoints**
- `admin_deactivate_user`, `deactivated_user_cannot_login`, `deactivated_user_token_revoked`, and 11 more
- Only check status code; broken response data would not be caught

**T11: Audit log + list tests use `>= N` instead of `== N`**
- `admin_actions_create_audit_log`: `assert!(row.0 >= 1)` — hides duplicate entries
- `admin_list_users`: `>= 3` — with isolated DB, should be exactly 3
- 8+ tests across admin, issue_mr, project files

**T12: Webhook payload content never verified** (`tests/webhook_integration.rs::webhook_fires_on_issue_create`)
- Only checks mock received 1 request; doesn't inspect body or event type
- Compare with `webhook_hmac_signature` which correctly inspects payload

**T13: `read_project_secret()` decryption path untested** (`src/api/secrets.rs:288-326`)
- Returns **decrypted** secret value — security-sensitive path
- No integration test calls GET `/api/projects/{id}/secrets/{name}`

**T14: Secret encryption round-trip not verified via API** (`tests/secrets_integration.rs`)
- Tests verify value is hidden in list, but never read back the decrypted value
- `query_scoped_secrets` tests don't assert decrypted value matches original

**T15: No soft-deleted project resource access tests** (`tests/project_integration.rs`)
- `delete_project_soft_delete` verifies GET returns 404
- Never tests: create issue/pipeline/webhook on deleted project

**T16: No workspace-derived permission tests** (`tests/rbac_integration.rs`)
- CLAUDE.md documents workspace → project permission derivation
- No integration test verifies member gets implicit ProjectRead

**T17: 4 flaky sleep-based tests (observe ingest)**
- `observe_ingest_integration.rs`: 4× `sleep(1500ms)` waiting for flush tasks
- Should use signal-driven synchronization

**T18: 4 flaky sleep-based tests (webhook dispatch)**
- `webhook_integration.rs`: 4× `sleep(3s)`, 1× `sleep(8s)` for async delivery
- Should poll `mock_server.received_requests()` with timeout

**T19: Flaky sleep in deployment preview cleanup** (`deployment_integration.rs:896`)
- `sleep(2s)` waiting for async cleanup — race condition

**T20: Flaky sleep in auth token update** (`auth_integration.rs:868`)
- `sleep(200ms)` for fire-and-forget DB update

**T21: `body_json` helper silently returns `Value::Null` on parse failure** (`tests/helpers/mod.rs`)
- `serde_json::from_slice(&bytes).unwrap_or(Value::Null)`
- Should `expect("response body is not valid JSON")` to fail fast

**T22: Feature flags CRUD endpoints untested** (`src/api/flags.rs`)
- 10+ handlers: create, toggle, add_rule, delete_rule, set_override, history
- Only flag evaluation tested (in `deployment_integration.rs`)

**T23: Permission cache TTL not set in test helpers** (`tests/helpers/mod.rs`)
- Production calls `set_cache_ttl(300)` — test helpers skip this
- Tests may exercise different caching behavior than production

**T24: `dashboard_stats_with_data` documents a known bug but doesn't track it** (`tests/dashboard_integration.rs`)
- Comment: "pipeline_runs table doesn't exist (known bug)"
- No assertion codifies expected vs actual behavior

**T25: Token expiry boundary tests missing** (`tests/auth_integration.rs`)
- No test for `expires_in_days=0`, `-1`, or `366` (above max 365)

---

### MEDIUM — Fix when touching the area

**T26: ~26 unit tests in `src/deployer/ops_repo.rs` do filesystem+git I/O**
- Should be integration tier — inflates unit coverage, slows `just test-unit`
- Affected: `init_and_get_sha_roundtrip`, `commit_values_creates_file`, etc. (23 tests)

**T27: 6 unit tests in `src/git/repo.rs` do filesystem+git I/O**
- `init_bare_repo_creates_directory`, `init_bare_repo_custom_branch`, etc.
- Should be integration tier

**T28: 3 unit tests in `src/onboarding/claude_auth.rs` spawn subprocesses**
- `pty_flow_url_extraction`, `pty_flow_full_code_to_token`, `pty_flow_manager_start_auth`
- Should be integration tier

**T29: 6 E2E tests in `e2e_agent.rs` should be integration** (single-endpoint tests)
- `agent_session_creation`, `agent_session_with_custom_image`, `agent_role_determines_mcp_config`
- Per decision tree: single endpoint + K8s pod side effect = integration

**T30: 5 integration tests are multi-endpoint journeys** (`deployment_integration.rs`)
- `canary_release_full_lifecycle`, `canary_rollback_lifecycle`, `preview_cleanup_on_mr_merge`
- `flags_registered_and_evaluable_from_ops_repo`, `demo_project_creates_mr_and_triggers_pipeline`
- Span 3-6 endpoints across multiple domains

**T31: Test router missing git protocol routes** (`tests/helpers/mod.rs::test_router`)
- Production merges `git::git_protocol_router()` — test router doesn't
- Any new integration test for git push/pull would get 404

**T32: Test router missing OTLP ingest routes**
- Requires `IngestChannels` from `observe::router()`
- Workaround exists (`observe_pipeline_test_router()`)

**T33: Test RBAC missing HPA, PDB, pods/attach** (`hack/test-manifests/rbac.yaml`)
- Production ClusterRole includes `autoscaling`, `policy` apiGroups
- Deployer tests would fail for manifests with these resources

**T34: Mock CLI covers only 3 of 12+ ProgressKind variants**
- Missing: `Thinking`, `ToolCall`, `ToolResult`, `Error`, `WaitingForInput`, `IframeAvailable`, etc.
- Full flow of these event types untested in integration

**T35: Mock CLI missing `error` event type** (`tests/fixtures/mock-claude-cli.sh`)
- No integration test validates error event parsing from CLI subprocess

**T36: Squash/rebase merge strategies untested** (`src/api/merge_requests.rs`)
- `git_squash_merge()` and `git_rebase_merge()` have zero test coverage
- Only default no-ff merge tested

**T37: Stale review dismissal untested**
- `dismiss_stale_reviews` in branch protection never exercised
- No test: push commit → existing reviews marked stale → merge blocked

**T38: SSE endpoints untested** (`src/api/sessions.rs`, `src/api/pipelines.rs`, `src/health.rs`)
- `sse_session_events`, `sse_session_events_global`, `stream_live_logs`, `health_sse`
- No integration test verifies SSE streaming

**T39: ~20 error-path tests only check status code** (across all files)
- Don't verify error message content or JSON structure
- If error format changes, tests won't catch it

**T40: Ops repo update/delete untested** (`src/api/deployments.rs`)
- DELETE has 409 Conflict check for active references — untested

**T41: Flaky sleep in pubsub tests** (`tests/pubsub_integration.rs`)
- 7× `sleep(50ms)` — relatively safe but not deterministic

**T42: Flaky 15s sleep in E2E deployer** (`tests/e2e_deployer.rs:336`)
- Waiting for preview TTL expiry — consumes half the per-test timeout budget

**T43: SSRF tests missing DNS rebinding** (`tests/webhook_integration.rs`)
- No test for `[::ffff:127.0.0.1]` or hex-encoded IPs

**T44: Pipeline trigger edge cases** (`tests/pipeline_integration.rs`)
- No test for empty `git_ref` string
- No test for cancel by non-project-member

**T45: Registry edge cases** (`tests/registry_integration.rs`)
- No test for zero-byte blob, empty layers, excessive layer count

**T46: Issue comment empty body not tested** (`tests/issue_mr_integration.rs`)
- MR comment empty body test exists, but not issue comment equivalent

**T47: 9 `#[allow(dead_code)]` in E2E test files**
- `e2e_pipeline.rs`, `e2e_deployer.rs`, `e2e_demo.rs`, `e2e_gitops.rs`
- May indicate stale helper code

---

### LOW — Fix if trivial

**T48:** `admin_list_delegations` checks `!is_empty()` instead of exact count
**T49:** `admin_list_service_accounts` uses `>= 1` instead of `== 1`
**T50:** Missing timestamp recency checks on `created_at`/`updated_at` in CRUD tests
**T51:** Test naming — `create_project`, `create_issue` not descriptive enough for CI reports
**T52:** `store/commands_seed.rs` unit test does filesystem I/O (borderline acceptable)
**T53:** E2E `test_router()` much sparser than integration `test_router()`
**T54:** Mock CLI missing `total_cost_usd`, `duration_ms` fields (Optional, tested in unit)
**T55:** Direct Valkey key deletion `rate:setup:global` in `setup_integration.rs`
**T56:** Duplicate email user creation not tested (only duplicate name)
**T57:** Passkey edge cases likely missing (expired credential, duplicate device)
**T58:** Missing security header tests (X-Frame-Options, X-Content-Type-Options)
**T59:** Missing test router layers: CORS, tracing middleware, UI fallback

---

## Summary Statistics

| Severity | Count |
|---|---|
| CRITICAL | 9 |
| HIGH | 16 |
| MEDIUM | 22 |
| LOW | 12 |
| **Total** | **59** |

## Top 5 Actions

1. **Create integration test files for 5 untested API modules** (releases, branch_protection, llm_providers, onboarding, flags CRUD) — CRITICAL, covers ~40 handlers
2. **Add `enforce_merge_gates()` tests** — CRITICAL security gap, the primary merge safety mechanism
3. **Fix 3 false-positive tests** (A2/T4, A3/T5, A1/T6) — tests pass with broken code
4. **Replace 10+ sleep-based waits with polling** — HIGH flakiness risk across observe, webhook, deployment, auth tests
5. **Add input validation edge cases** (Unicode, null bytes, pagination boundaries) — CRITICAL, zero coverage on API boundary validation
