# Plan 28 — Unit Test Coverage for Untested Logic

## Context

Unit coverage sits at 46.5% (442 tests). The well-tested modules — validation (99%), pipeline/definition (99%), rbac/types (98%) — prove the pattern works. But several modules with real branching logic, state machines, and transformations have 0-50% unit coverage, relying entirely on integration/E2E tests for validation.

This plan targets **logic that makes decisions**, not DB wiring or HTTP handler glue. Functions that are "call DB, return result" belong to integration tests and are excluded.

---

## Priority 1 — Critical (0% coverage, core business logic)

### 1.1 `pipeline/trigger.rs` (0%)

All pipeline triggering paths are untested. The functions shell out to git, but the logic around them is testable.

**Testable pure logic to extract and test:**
- `read_file_at_ref()` — git show wrapper, test failure→None and success→Some(content)
- `get_ref_sha()` — git rev-parse wrapper, test whitespace trimming, failure→None
- Trigger matching in `on_push()` — given a parsed `PipelineDefinition` and a branch ref, does `matches_push()` return true? (already tested in definition.rs, but the integration in trigger.rs is not)
- Ref formatting: `refs/heads/main` → `main` extraction logic

**Tests to add (~6 tests):**
```
trigger_read_file_returns_none_on_missing_ref
trigger_read_file_returns_content
trigger_get_ref_sha_trims_whitespace
trigger_get_ref_sha_returns_none_on_failure
trigger_ref_to_branch_strips_prefix
trigger_on_push_skips_when_no_pipeline_yaml
```

### 1.2 `auth/rate_limit.rs` (0%)

Rate limiting is security-critical. Currently depends entirely on Valkey, so unit testing requires either extracting the threshold logic or using a test Valkey.

**Testable logic to extract:**
- Threshold check: `count > max` → error. Extract as a pure function.
- Window expiry: only set TTL when `count == 1` (first request in window).

**Tests to add (~4 tests):**
```
rate_limit_first_request_allowed
rate_limit_at_threshold_allowed
rate_limit_over_threshold_rejected
rate_limit_error_type_is_too_many_requests
```

**Approach:** Extract `check_rate_result(count: i64, max: i64) -> Result<(), ApiError>` as a pure function, test it directly. The Valkey interaction stays in the async wrapper.

### 1.3 `notify/webhook.rs` (0% unit)

Webhook delivery has HMAC signing logic that's pure and testable.

**Testable pure logic:**
- HMAC-SHA256 computation: given (payload, secret) → signature string
- Header construction: `X-Platform-Signature: sha256={hex}`
- Concurrency semaphore acquire failure path

**Tests to add (~5 tests):**
```
webhook_hmac_signature_matches_expected
webhook_hmac_deterministic
webhook_no_signature_header_without_secret
webhook_signature_format_sha256_prefix
webhook_payload_serialization_is_compact
```

### 1.4 `agent/identity.rs` (0% unit)

Identity creation/cleanup is all DB operations — not a good unit test target. **Skip for unit tests.** The E2E tests (`e2e_agent.rs`) already cover the full lifecycle. Integration tests are the right tier here.

### 1.5 `observe/alert.rs::handle_alert_state()` (0%)

Alert state machine transitions are pure logic once extracted from the DB context.

**Testable state transitions:**
- `inactive` + condition met → `pending` (record first_triggered_at)
- `pending` + held_for >= for_seconds → `firing` (dispatch notification)
- `firing` + condition no longer met → `resolved`
- `pending` + condition no longer met → `inactive` (reset)

**Tests to add (~6 tests):**
```
alert_inactive_to_pending_on_condition_met
alert_pending_to_firing_after_hold_period
alert_pending_resets_when_condition_clears
alert_firing_resolves_when_condition_clears
alert_firing_stays_firing_while_condition_holds
alert_already_firing_no_duplicate_notification
```

**Approach:** Extract state transition as `fn next_alert_state(current: &str, condition_met: bool, held_for_secs: i64, for_seconds: i64) -> (&str, bool)` where the bool indicates "should notify". Test the pure function.

---

## Priority 2 — High (logic with <50% unit coverage)

### 2.1 `secrets/engine.rs` — template substitution (untested)

`resolve_secrets_for_env()` does regex-like pattern matching on `${{ secrets.NAME }}` templates. This is pure string transformation once the DB lookup is abstracted.

**Tests to add (~7 tests):**
```
secret_template_single_substitution
secret_template_multiple_substitutions
secret_template_no_patterns_returns_unchanged
secret_template_invalid_name_rejected
secret_template_missing_secret_returns_error
secret_template_nested_braces_handled
secret_template_adjacent_patterns
```

**Approach:** Extract pattern matching into `fn extract_secret_names(template: &str) -> Vec<&str>` and test it. The DB lookup stays in the async wrapper.

### 2.2 `secrets/engine.rs` — hierarchical resolution (untested)

`resolve_secret_hierarchical()` implements a 4-level fallback: project+env → project → workspace → global.

**Not a unit test target** — this is a single SQL query with `ORDER BY CASE`. Test via integration tests instead. The ordering logic lives in SQL, not Rust.

### 2.3 `pipeline/executor.rs` — build_env_vars (untested)

Constructs environment variables for pipeline pods. Pure function.

**Tests to add (~4 tests):**
```
env_vars_include_all_seven_standard_vars
env_vars_commit_sha_none_omits_var
env_vars_registry_url_fallback_when_none
env_vars_branch_from_ref_strips_prefix
```

### 2.4 `pipeline/executor.rs` — detect_and_write_deployment (untested)

Auto-detects kaniko builds and creates deployment records. Has real branching logic.

**Testable pure logic to extract:**
- Kaniko image detection: step image contains "kaniko" or "gcr.io/kaniko-project"
- Branch classification: is it main/master → production, else → preview
- Image ref construction: registry/project-name:tag

**Tests to add (~5 tests):**
```
detect_kaniko_image_standard
detect_kaniko_image_gcr_prefix
detect_kaniko_image_false_for_alpine
branch_main_classified_as_production
branch_feature_classified_as_preview
```

### 2.5 `rbac/resolver.rs` — add_workspace_permissions (untested)

Derives project permissions from workspace membership roles.

**Not a unit test target** — this is DB query logic (find workspace membership, map role→permissions). Test via integration tests.

### 2.6 `observe/alert.rs` — evaluate_metric (untested)

Each aggregation function (avg, sum, max, min, count) applied to metric data.

**Not a unit test target** — this is a SQL query with aggregation. The `check_condition()` function that evaluates the result IS testable and already has tests. Add edge cases:

**Tests to add (~4 tests):**
```
check_condition_nan_returns_false
check_condition_infinity_gt_threshold
check_condition_absent_with_none_threshold
check_condition_unknown_op_returns_false
```

---

## Priority 3 — Medium (edge cases in partially-tested modules)

### 3.1 `rbac/resolver.rs` — scope_allows edge cases

**Tests to add (~3 tests):**
```
scope_allows_case_sensitive_matching
scope_allows_duplicate_scopes_in_list
scope_allows_partial_match_rejected
```

### 3.2 `deployer/renderer.rs` — split_yaml edge cases

**Tests to add (~3 tests):**
```
split_yaml_windows_line_endings
split_yaml_indented_separator_not_split
split_yaml_unicode_content_preserved
```

### 3.3 `deployer/preview.rs` — build edge cases

**Tests to add (~3 tests):**
```
preview_namespace_truncation_strips_trailing_dash
preview_deployment_resource_limits_present
preview_service_port_mapping_80_to_8080
```

### 3.4 `auth/middleware.rs` — extraction edge cases

**Tests to add (~3 tests):**
```
extract_bearer_empty_token_rejected
extract_session_cookie_multiple_cookies_parsed
extract_ip_ipv6_x_forwarded_for
```

---

## Implementation Strategy

### Extract-and-test pattern

For functions that mix pure logic with DB/K8s calls, extract the pure part:

```rust
// Before: one big async function
async fn on_push(state: &AppState, project_id: Uuid, refs: &[PushRef]) -> Result<()> {
    let yaml = read_file_at_ref(&repo_path, &ref_name, ".platform.yaml").await?;
    let def = parse_pipeline_yaml(&yaml)?;
    if !def.matches_push(&branch) { return Ok(()); }
    // ... create pipeline in DB ...
}

// After: extract testable logic
fn ref_to_branch(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

fn should_trigger_push(def: &PipelineDefinition, branch: &str) -> bool {
    def.matches_push(branch)
}

// Unit test the extracted functions, integration test the full flow
```

### Test file organization

Add tests as `#[cfg(test)] mod tests` blocks at the bottom of each source file. Do not create separate test files for unit tests.

### Order of implementation

1. **Priority 1** first — these are the highest-value gaps
2. Within Priority 1, start with `auth/rate_limit.rs` (smallest, quickest win)
3. Then `notify/webhook.rs` (HMAC signing is pure)
4. Then `observe/alert.rs` state machine
5. Then `pipeline/trigger.rs` (needs some extraction)
6. **Priority 2** next — `secrets/engine.rs` template substitution, then executor env vars
7. **Priority 3** last — edge cases, add opportunistically

### Expected impact

| Tier | Tests to add | Estimated coverage lift |
|---|---|---|
| Priority 1 | ~21 tests | +3-4% overall |
| Priority 2 | ~20 tests | +2-3% overall |
| Priority 3 | ~12 tests | +1% overall |
| **Total** | **~53 tests** | **+6-8% → ~52-54%** |

The goal is not to maximize the coverage number. It's to ensure every piece of **decision-making logic** has a fast, isolated test that localizes failures.

---

## What NOT to unit test (explicitly excluded)

- `api/*.rs` handlers — thin glue, tested by integration tests
- `store/*.rs` — connection setup, tested by E2E
- `agent/identity.rs` — all DB operations, tested by E2E
- `agent/service.rs` — K8s pod orchestration, tested by E2E
- `deployer/reconciler.rs` — continuous loop with K8s, tested by E2E
- `deployer/applier.rs` — kubectl apply equivalent, already well-tested (100% on parse functions)
- `observe/ingest.rs` — OTLP HTTP endpoint wiring
- `observe/store.rs` — Parquet/MinIO operations
- `observe/query.rs` — SQL query construction
- `secrets/engine.rs::resolve_secret_hierarchical()` — SQL ordering logic, integration test target
- `rbac/resolver.rs::add_workspace_permissions()` — DB query logic, integration test target
