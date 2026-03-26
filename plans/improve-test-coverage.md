# Plan: Improve Test Coverage (79.67% → 85%+)

## Current State (2026-03-26)
- **Overall coverage**: 79.67% line coverage (32,349 / 40,603 lines)
- **Target**: 85% = cover ~2,200 more lines
- **Stretch**: 87% = cover ~3,000 more lines
- **2 pre-existing test failures**: `auto_setup_downloads_agent_runner`, `pull_platform_runner_bare_image` (registry seed issues, unrelated)

## Progress Log

| Module | Action | Tests Added | Lines Impact | Date |
|--------|--------|-------------|-------------|------|
| `pipeline/executor.rs` | Deleted ~300 lines of dead code (`detect_and_write_deployment`, `gitops_handoff`, `write_file_to_ops_repo`, `detect_and_publish_dev_image`, `upsert_preview_deployment`) + removed unused `DEV_IMAGE_STEP_NAME` constant | 0 | ~300 lines removed from denominator | 2026-03-26 |
| `pipeline/executor.rs` | Added 20 new unit tests: `SHORT_SHA`/`IMAGE_TAG`/`VERSION` env vars, `git_secret_name` pod spec mount, combined secrets pod spec, `mark_transitive_dependents` edge cases, `detect_unrecoverable_container` edge cases, `step_condition_from_row` branches | 20 | ~50 new lines covered | 2026-03-26 |
| `pipeline/executor.rs` | **Tier migration**: moved 9 single-endpoint executor tests from `e2e_pipeline.rs` → `executor_integration.rs` (integration tier). Added `poll_pipeline_status` + `start_pipeline_server` (random port, parallel-safe) to `helpers/mod.rs`. Kept only `concurrent_pipeline_limit` (multi-pipeline journey) in E2E. | 9 moved | ~500 lines now in cov-total | 2026-03-26 |
| `pipeline/trigger.rs` | Added 9 unit tests: `is_valid_semver` (5), `increment_patch_from_zero`, `parse_version_file` edge cases (3). Added 12 integration tests: `on_tag` (3), `read_file_at_ref` direct (3), `read_dir_at_ref` (3), `read_version_at_ref` (2), trigger filter test. | 9 unit + 12 int | ~130 lines covered | 2026-03-26 |

---

## Top Coverage Gaps (sorted by missed lines)

| File | Lines | Missed | Cover | Testability |
|------|-------|--------|-------|-------------|
| pipeline/executor.rs | ~2990 | ~900 (est.) | ~70% (est.) | K8s-bound async now covered by integration tests |
| deployer/reconciler.rs | 1110 | 799 | 28.0% | Phase handlers need K8s; DB-only helpers testable |
| agent/service.rs | 655 | 388 | 40.8% | Session lifecycle, heavy async |
| agent/llm_validate.rs | 516 | 381 | 26.2% | **LLM tier only** — real Claude API |
| agent/create_app.rs | 549 | 287 | 47.7% | **LLM tier only** — mock CLI tool loop |
| api/merge_requests.rs | 1073 | 272 | 74.7% | Git merge strategies testable with bare repos |
| deployer/analysis.rs | 267 | 246 | 7.9% | Async tick/analysis, needs K8s state |
| api/onboarding.rs | 493 | 240 | 51.3% | Claude auth flows need PTY; wizard paths testable |
| pipeline/trigger.rs | 547 | 216 | 60.5% | Version bump + tag trigger testable |
| health/checks.rs | 329 | 214 | 35.0% | Async probes testable with real cluster |
| observe/tracing_layer.rs | 288 | 170 | 41.0% | Layer impl needs subscriber harness |
| onboarding/claude_auth.rs | 518 | 169 | 67.4% | PTY/process spawning — mostly deferred |
| api/deployments.rs | 903 | 134 | 85.2% | Close to target already |
| deployer/ops_repo.rs | 803 | 120 | 85.1% | Close to target already |
| api/sessions.rs | 663 | 115 | 82.7% | Several untested handlers |
| api/admin.rs | 302 | 110 | 63.6% | Role/delegation edge cases |

---

## Phase 1: Unit Tests for Pure Functions (~180 lines covered)

Fast tests, no cluster, `just test-unit`.

### P1.1 — `pipeline/trigger.rs` pure helpers

| Function | Tests to add |
|----------|--------------|
| `parse_version_file()` | 3 tests: valid VERSION, empty, malformed |
| `is_valid_semver()` | 3 tests: valid, missing patch, invalid |
| `increment_patch()` | 2 tests: normal bump, from 0 |
| `ref_to_branch()` | 2 tests: refs/heads/X → X, bare ref |
| `should_trigger_push()` | 3 tests: branch match, mismatch, wildcard |

**Effort**: Low (13 tests, ~100 lines of test code)
**Lines covered**: ~50

### P1.2 — `api/sessions.rs` pure helpers

| Function | Tests to add |
|----------|--------------|
| `truncate_prompt()` | 2 tests: under limit, over limit |
| `validate_provider_config()` | 3 tests: valid, missing field, invalid type |

**Effort**: Low (5 tests, ~50 lines)
**Lines covered**: ~20

### P1.3 — `observe/tracing_layer.rs` visitor methods

Tests for `SpanFieldVisitor` and `FieldVisitor` using tracing's `field::Field` API.

| Function | Tests to add |
|----------|--------------|
| `SpanFieldVisitor::record_str()` | 3 tests: UUID fields, string fields, unknown field |
| `FieldVisitor::record_str()` | 2 tests: message field, other field |
| `FieldVisitor::record_i64()` | 1 test: numeric field |
| `FieldVisitor::record_debug()` | 1 test: debug-formatted message |
| `create_channel()` | 1 test: returns sender/receiver |

**Effort**: Medium (8 tests, ~80 lines)
**Lines covered**: ~40

### P1.4 — `error.rs` remaining paths

| Function | Tests to add |
|----------|--------------|
| `ApiError::Forbidden` body | 1 test |
| `ApiError::Unauthorized` body | 1 test |
| `ApiError::TooManyRequests` body | 1 test |
| `ApiError::Conflict` body | 1 test |

**Effort**: Low (4 tests, ~30 lines)
**Lines covered**: ~15

### P1.5 — `pipeline/executor.rs` remaining pure functions

| Function | Tests to add |
|----------|--------------|
| `build_pod_spec()` with imagebuild | 2 tests: kaniko step, deploy-test step |
| `build_pod_spec()` with setup_commands | 1 test: init container with setup |

**Effort**: Low (3 tests, ~50 lines)
**Lines covered**: ~30

### P1.6 — `deployer/reconciler.rs` — `lookup_stable_image` pattern

| Function | Tests to add |
|----------|--------------|
| `handle_rolling_progress()` | 1 test: returns Ok (trivial) |

**Effort**: Minimal (1 test, ~10 lines)
**Lines covered**: ~5

---

## Phase 2: Integration Tests for API Handlers (~900 lines covered)

Requires dev cluster. Run with `just test-integration`.

### P2.1 — `api/sessions.rs` untested handlers (~80 lines)

| Handler | Tests to add |
|---------|--------------|
| `list_iframes` | 1 test: returns empty then with data |
| `get_session_progress` | 1 test: returns messages for session |
| `replay_stored_events` | 1 test: replays historical messages |
| `sse_session_events_global` | 1 test: global SSE stream |

**Effort**: Medium (4 tests, ~120 lines)
**Lines covered**: ~80

### P2.2 — `api/admin.rs` remaining handlers (~70 lines)

| Handler | Tests to add |
|---------|--------------|
| `update_role` (name + description) | 1 test |
| `get_role` single | 1 test |
| `get_delegation` single | 1 test |
| `get_service_account` single | 1 test |

**Effort**: Low (4 tests, ~80 lines)
**Lines covered**: ~70

### P2.3 — `api/issues.rs` edge cases (~50 lines)

| Handler | Tests to add |
|---------|--------------|
| `create_issue` validation error (empty title) | 1 test |
| `update_issue` label edge cases (max labels) | 1 test |
| `delete_comment` by non-author non-admin | 1 test |

**Effort**: Low (3 tests, ~60 lines)
**Lines covered**: ~50

### P2.4 — `health/checks.rs` async probes (~100 lines)

Integration tests using real cluster infra.

| Probe | Tests to add |
|-------|--------------|
| `build_snapshot()` returns all 7 subsystems | 1 test (via health API detail endpoint) |
| `query_pod_failures()` with data | 1 test: insert failed pod, verify returned |
| `is_ready()` true when healthy | 1 test (already partially covered by `readyz_returns_ok`) |

**Effort**: Medium (3 tests, ~80 lines)
**Lines covered**: ~100

### P2.5 — `pipeline/trigger.rs` — `on_tag` and version bump (~100 lines)

| Handler | Tests to add |
|---------|--------------|
| `on_tag` creates pipeline from tag push | 1 test |
| `auto_bump_version` increments VERSION file | 1 test |
| `read_file_at_ref` returns file content | 1 test |

**Effort**: High (3 tests, ~120 lines, requires git repo setup)
**Lines covered**: ~100

### P2.6 — `api/merge_requests.rs` — git merge operations (~150 lines)

Tests that exercise the actual git merge code paths (require bare repo + working copy).

| Handler | Tests to add |
|---------|--------------|
| `delete_mr` | 1 test: soft-delete MR |
| `create_mr` with duplicate source/target branch | 1 test |
| `list_comments` pagination | 1 test |
| `create_review` approve/request_changes | 1 test |

**Effort**: Medium (4 tests, ~120 lines)
**Lines covered**: ~150

### P2.7 — `api/onboarding.rs` — claude auth paths (~100 lines)

| Handler | Tests to add |
|---------|--------------|
| `start_claude_auth` returns session | 1 test |
| `claude_auth_status` polling | 1 test |
| `submit_auth_code` with invalid code | 1 test |
| `verify_oauth_token` success | 1 test |

**Effort**: Medium (4 tests, ~120 lines)
**Lines covered**: ~100

### P2.8 — `api/commands.rs` remaining gaps (~40 lines)

| Handler | Tests to add |
|---------|--------------|
| `create_command` with all optional fields | 1 test |
| `resolve_command` project-scoped fallback chain | 1 test |

**Effort**: Low (2 tests, ~40 lines)
**Lines covered**: ~40

### P2.9 — `store/eventbus.rs` — event handler paths (~50 lines)

| Handler | Tests to add |
|---------|--------------|
| `handle_event` FlagsRegistered creates flags | 1 test |
| `handle_event` DevImageBuilt updates agent image | 1 test |

**Effort**: Medium (2 tests, ~60 lines)
**Lines covered**: ~50

---

## Phase 3: Background Service Integration Tests (~400 lines covered)

Complex tests requiring K8s state + background tasks.

### P3.1 — `pipeline/executor.rs` DB-only helpers (~120 lines)

Test the DB-only helper functions that don't need K8s pods.

| Function | Tests to add |
|----------|--------------|
| `create_git_auth_token` + `cleanup_git_auth_token` | 2 tests: create/verify/cleanup token |
| `create_pipeline_otlp_token` | 1 test: creates OTEL token |
| `emit_pipeline_log` | 1 test: writes log entry |
| `is_cancelled` | 1 test: returns false for running pipeline |
| `skip_remaining_steps` | 1 test: marks pending steps as skipped |

**Effort**: Medium (6 tests, ~150 lines)
**Lines covered**: ~120

### P3.2 — `deployer/reconciler.rs` DB-only helpers (~80 lines)

| Function | Tests to add |
|----------|--------------|
| `transition_phase` | 2 tests: valid transition, invalid |
| `record_history` | 1 test: creates history entry |
| `cleanup_expired_previews` | 1 test: deletes aged targets |

**Effort**: Medium (4 tests, ~100 lines)
**Lines covered**: ~80

### P3.3 — `agent/service.rs` session lifecycle (~100 lines)

| Function | Tests to add |
|----------|--------------|
| `create_session` with parent | 1 test: spawn depth computed |
| `cleanup_session` deletes identity | 1 test |
| `list_active_sessions` | 1 test |

**Effort**: Medium (3 tests, ~80 lines)
**Lines covered**: ~100

### P3.4 — `deployer/namespace.rs` + `deployer/ops_repo.rs` gaps (~100 lines)

| Function | Tests to add |
|----------|--------------|
| `ensure_project_namespace` idempotent | 1 test |
| `sync_from_project_repo` with env vars | 1 test |

**Effort**: High (2 tests, ~80 lines, complex K8s setup)
**Lines covered**: ~100

---

## Phase 4: Deferred / Out of Scope

| File | Lines Missed | Reason |
|------|-------------|--------|
| agent/llm_validate.rs | 381 | LLM test tier only (real Claude API) |
| agent/create_app.rs | 287 | LLM test tier only (mock CLI tool loop) |
| git/ssh_server.rs | 190 | SSH protocol hard to test |
| observe/tracing_layer.rs (Layer impl) | ~100 | Requires tracing subscriber harness |
| onboarding/claude_auth.rs (PTY flow) | ~120 | PTY/process spawning |
| deployer/analysis.rs | 246 | Complex async analysis tick |
| registry/gc.rs | 25 | Background GC loop |
| store/pool.rs | 3 | Trivial re-export |

---

## Implementation Order

| Order | Item | Module | Type | Est. Lines Covered | Effort |
|-------|------|--------|------|-------------------|--------|
| 1 | P1.1 | pipeline/trigger.rs | Unit | 50 | Low |
| 2 | P1.2 | api/sessions.rs | Unit | 20 | Low |
| 3 | P1.3 | observe/tracing_layer.rs | Unit | 40 | Medium |
| 4 | P1.4 | error.rs | Unit | 15 | Low |
| 5 | P1.5 | pipeline/executor.rs | Unit | 30 | Low |
| 6 | P1.6 | deployer/reconciler.rs | Unit | 5 | Minimal |
| 7 | P2.1 | api/sessions.rs | Integration | 80 | Medium |
| 8 | P2.2 | api/admin.rs | Integration | 70 | Low |
| 9 | P2.3 | api/issues.rs | Integration | 50 | Low |
| 10 | P2.4 | health/checks.rs | Integration | 100 | Medium |
| 11 | P2.5 | pipeline/trigger.rs | Integration | 100 | High |
| 12 | P2.6 | api/merge_requests.rs | Integration | 150 | Medium |
| 13 | P2.7 | api/onboarding.rs | Integration | 100 | Medium |
| 14 | P2.8 | api/commands.rs | Integration | 40 | Low |
| 15 | P2.9 | store/eventbus.rs | Integration | 50 | Medium |
| 16 | P3.1 | pipeline/executor.rs | Integration | 120 | Medium |
| 17 | P3.2 | deployer/reconciler.rs | Integration | 80 | Medium |
| 18 | P3.3 | agent/service.rs | Integration | 100 | Medium |
| 19 | P3.4 | deployer/namespace+ops_repo | Integration | 100 | High |

---

## Expected Coverage Impact

| Phase | Tests | Lines Covered | Cumulative |
|-------|-------|--------------|-----------|
| Baseline | — | — | 79.67% |
| Phase 1 (Unit) | ~34 | ~180 | 80.1% |
| Phase 2 (Integration) | ~30 | ~900 | 82.3% |
| Phase 3 (Background) | ~15 | ~400 | 83.3% |

**Note**: These estimates are conservative. Integration tests often cover shared code paths (middleware, auth, validation) beyond the targeted handler, so actual coverage gain will likely be higher.

**Conservative target**: 83%+ after all three phases.
**Stretch**: If integration tests cover more shared paths than estimated, 85%+ is achievable.

To reach 85%+ without testing the deferred LLM/SSH/PTY paths, consider also adding:
- More edge-case tests for existing well-covered modules (api/webhooks, api/flags, auth/middleware)
- Negative-path tests (malformed input, auth failures) that exercise error branches

---

## Key Principles

1. **Unit tests first** — P1.1-P1.6 run with `just test-unit` (~1s), no cluster
2. **Integration tests follow existing patterns** — `helpers::test_state()` + `helpers::test_router()`
3. **Never mock kube client** — always use real Kind cluster
4. **Run targeted tests during dev** — `cargo nextest run --lib -E 'test(name)'` for unit, `just test-bin module test_name` for integration
5. **Run `just ci-full` once at the end** before declaring each phase complete
6. **DB-only helpers are high ROI** — P3.1 and P3.2 test pipeline/deployer helpers that only need Postgres, not K8s pods
