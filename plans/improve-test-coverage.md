# Plan: Improve Unit + Integration Test Coverage

## Current State
- **Overall coverage**: 77.65% line coverage across unit + integration tiers
- **Total missed lines in scope**: ~5,800 lines across all targeted modules
- **Target**: 85%+ after Phases 1+2, stretch 87%+ after Phase 3

---

## Phase 1: Unit Tests for Pure Functions (est. +440 lines covered)

Fast tests, no cluster required, highest ROI.

### P1.1 — `pipeline/executor.rs` (1805 missed lines, 40.70% → ~47%)

Existing tests cover `slug()`, `extract_exit_code()`, `build_pod_spec()`. Untested pure functions:

| Function | What it does | Tests to add |
|----------|-------------|--------------|
| `check_container_statuses()` | Detects ImagePullBackOff, CrashLoopBackOff | 8 tests: each error type, running/OK, empty list |
| `detect_unrecoverable_container()` | Combines init + regular container checks | 2 tests: init container failure, no statuses |
| `build_volumes_and_mounts()` | Volume/mount construction | 4 tests: no secrets, registry only, git only, both |
| `build_env_vars_core()` | Core env var builder (no AppState) | 3 tests: all fields, without SHA, with registry |
| `is_reserved_pipeline_env_var()` | Simple lookup | 2 tests: true/false cases |
| `container_security()` | Returns hardened SecurityContext | 1 test: drops all caps |
| `node_registry_url()` | Config lookup | 3 tests: prefers node URL, falls back, returns None |
| `mark_transitive_dependents_skipped()` | DAG traversal (pure graph) | 3 tests: linear chain, diamond, already-completed |
| `step_condition_from_row()` | StepRow → StepCondition | 2 tests: empty arrays → None, with events |
| `extract_branch()` | Strip refs/heads/ prefix | 3 tests: refs/heads/, refs/tags/, bare |
| `build_pod_spec` extensions | With git secret mount, imagebuild | 2 tests |

**Effort**: Medium (33 tests, ~300 lines of test code)
**Lines covered**: ~250

### P1.2 — `observe/tracing_layer.rs` (186 missed lines, 0.00% → ~43%)

Entire file untested. Testable pure functions:

| Function | Tests to add |
|----------|--------------|
| `SpanFields::merge()` | 3 tests: fills gaps, no overwrite, empty source |
| `classify_source_from_target()` | 4 tests: api, auth, pipeline, system default |
| `SpanFieldVisitor::record_str()` | 2 tests: well-known fields, unknown fields |
| `FieldVisitor::record_str/debug/i64` | 2 tests: message extraction, numeric fields |

**Effort**: Medium (11 tests, ~150 lines)
**Lines covered**: ~80

### P1.3 — `agent/llm_validate.rs` (381 missed lines, 26.16% → ~32%)

Existing tests cover `build_provider_extra_env()`. Extension:

| Function | Tests to add |
|----------|--------------|
| `build_provider_extra_env()` edge cases | 4 tests: custom endpoint, Azure Foundry, Vertex no-dup, Anthropic key |
| `push_if_missing()` | 2 tests: already present, adds new |
| Serialization | 2 tests: TestResult, ValidationEvent |

**Effort**: Low (8 tests, ~60 lines)
**Lines covered**: ~30

### P1.4 — `store/eventbus.rs` (192 missed lines, 77.12% → ~78%)

One pure function testable: `resolve_deploy_config_from_specs()`.

| Function | Tests to add |
|----------|--------------|
| `resolve_deploy_config_from_specs()` | 5 tests: no deploy section, canary on staging, canary not on prod, A/B on staging, rolling defaults |

**Effort**: Low (5 tests, ~60 lines)
**Lines covered**: ~30

### P1.5 — `agent/create_app.rs` (399 missed lines, 27.59% → ~30%)

Existing tests cover `parse_create_project_input()`. More edge cases:

| Function | Tests to add |
|----------|--------------|
| `parse_create_project_input()` | 3 tests: with display_name+description, missing name, empty name |
| `LoopOutcome` | 1 test: equality/debug |

**Effort**: Low (4 tests, ~40 lines)
**Lines covered**: ~15

### P1.6 — `deployer/reconciler.rs` (790 missed lines, 25.89% → ~27%)

Existing tests cover `generate_basic_manifest()`, `target_namespace()`, docker config. Extend:

| Function | Tests to add |
|----------|--------------|
| `env_suffix()` | 3 tests: production→prod, staging→stg, custom |

**Effort**: Low (3 tests, ~20 lines)
**Lines covered**: ~15

### P1.7 — `error.rs` (89 missed lines, 68.55% → ~74%)

Error conversion paths:

| Function | Tests to add |
|----------|--------------|
| `From<ValidationError>` | 1 test |
| `BadGateway` body | 1 test: contains message |
| `ServiceUnavailable` body | 1 test: contains message |

**Effort**: Low (3 tests, ~30 lines)
**Lines covered**: ~15

### P1.8 — `deployer/analysis.rs` (246 missed lines, 7.87% → ~9%)

Only `invert_condition()` tested. Add edge cases:

| Function | Tests to add |
|----------|--------------|
| `invert_condition()` edge cases | 2 tests: LTE unchanged, empty string |

**Effort**: Minimal (2 tests, ~15 lines)
**Lines covered**: ~5

---

## Phase 2: Integration Tests for API Handlers (est. +1,080 lines covered)

Requires dev cluster (Postgres, Valkey, MinIO, K8s).

### P2.1 — `api/onboarding.rs` (351 missed lines, 30.77% → ~70%)

Major untested paths:

| Handler | Tests to add |
|---------|--------------|
| `complete_wizard` with `startup`/`tech_org` org types | 2 tests: team workspace creation |
| `complete_wizard` with `passkey_policy`, `cli_token`, `custom_provider` | 3 tests |
| `update_settings` | 2 tests: change org_type, non-admin forbidden |
| `get_settings` | 1 test: returns all fields |
| `create_demo_project` | 2 tests: returns project, non-admin forbidden |
| `verify_oauth_token` | 1 test: invalid token |
| Claude OAuth error paths | 2 tests: start_claude_auth error, cancel_not_found |
| Permission enforcement | 1 test: all endpoints require admin |

**Effort**: High (14 tests, ~400 lines)
**Lines covered**: ~200

### P2.2 — `api/merge_requests.rs` (409 missed lines, 61.08% → ~80%)

Untested paths:

| Handler | Tests to add |
|---------|--------------|
| `create_mr` with `auto_merge` | 1 test |
| `enable_auto_merge` / `disable_auto_merge` | 2 tests |
| `update_comment` / `delete_comment` | 2 tests |
| `get_review` (single) | 1 test |
| Close and reopen MR | 1 test |
| `merge_mr` validation (already merged, source branch) | 2 tests |
| Permission enforcement on write ops | 1 test |

**Effort**: High (10 tests, ~350 lines)
**Lines covered**: ~200

### P2.3 — `health/checks.rs` (266 missed lines, 5.67% → ~60%)

All probes untested:

**Unit tests** (new `mod tests` in checks.rs):
| Probe | Tests |
|-------|-------|
| `check_git_repos` | 2: exists, missing |
| `check_secrets_engine` | 3: with key, dev mode, no key |
| `check_registry` | 2: configured, not configured |
| `elapsed_ms` | 1: small duration |

**Integration tests** (extend health_integration.rs):
| Probe | Tests |
|-------|-------|
| Health endpoint snapshot | 1 test |
| Postgres probe | 1 test |
| Valkey probe | 1 test |
| MinIO probe | 1 test |
| K8s probe | 1 test |
| Pod failure summary | 2 tests: empty, with failures |

**Effort**: Medium (15 tests, ~250 lines)
**Lines covered**: ~150

### P2.4 — `api/admin.rs` (110 missed lines, 63.58% → ~90%)

Untested handlers:

| Handler | Tests to add |
|---------|--------------|
| `update_role` | 1 test: name and description |
| `delete_role` | 2 tests: custom role, system role forbidden |
| `set_role_permissions` | 1 test |
| `create_delegation` | 1 test |
| `list_delegations` | 1 test |
| `revoke_delegation` | 1 test |
| Admin token CRUD for other user | 3 tests: create, list, delete |

**Effort**: Medium (10 tests, ~250 lines)
**Lines covered**: ~80

### P2.5 — `api/passkeys.rs` (180 missed lines, 44.79% → ~63%)

| Handler | Tests to add |
|---------|--------------|
| `get_passkey` by ID | 1 test |
| `get_passkey` not found | 1 test |
| `begin_login` returns challenge | 1 test |
| `begin_login` no credentials | 1 test |
| Agent user cannot register | 1 test |
| `delete_passkey` not found | 1 test |
| `rename_passkey` validation | 1 test |

**Effort**: Medium (7 tests, ~150 lines)
**Lines covered**: ~60

### P2.6 — `api/commands.rs` (93 missed lines, 65.56% → ~80%)

| Handler | Tests to add |
|---------|--------------|
| Create command validation errors | 1 test |
| Update command not found | 1 test |
| Delete command not found | 1 test |
| Permission enforcement | 1 test |

**Effort**: Low (4 tests, ~80 lines)
**Lines covered**: ~40

### P2.7 — `pipeline/trigger.rs` (212 missed lines, 60.74% → ~88%)

| Handler | Tests to add |
|---------|--------------|
| `on_push` creates pipeline with steps | 1 test |
| `on_push` no platform.yaml → skip | 1 test |
| `on_push` branch mismatch → skip | 1 test |
| `on_mr` creates pipeline | 1 test |
| `on_mr` auto bumps version | 1 test |
| `on_tag` creates pipeline | 1 test |
| `on_api` creates pipeline | 1 test |
| `on_api` no platform.yaml → error | 1 test |

**Effort**: High (8 tests, ~300 lines, requires git repo setup)
**Lines covered**: ~150

---

## Phase 3: Integration Tests for Background Services (est. +750 lines covered)

### P3.1 — `deployer/reconciler.rs` phase handlers (790 missed → ~490 missed)

| Handler | Tests to add |
|---------|--------------|
| `handle_pending` rolling | 1 test: applies and completes |
| `handle_canary_progress` advance | 1 test: pass → next stage |
| `handle_canary_progress` hold | 1 test: fail → hold |
| `handle_canary_progress` rollback | 1 test: max failures → rollback |
| `handle_ab_test_progress` | 1 test: promotes after duration |
| `handle_promoting` | 1 test: finalizes release |
| `handle_rolling_back` | 1 test: reverts traffic |
| `cleanup_expired_previews` | 1 test: deletes old targets |

**Effort**: Very High (8 tests, ~500 lines, complex K8s + DB setup)
**Lines covered**: ~300

### P3.2 — `deployer/analysis.rs` async analysis (246 → ~146 missed)

| Handler | Tests to add |
|---------|--------------|
| `tick` evaluates progress gates | 1 test |
| `tick` detects rollback trigger | 1 test |
| `tick` inconclusive on low traffic | 1 test |
| `ensure_analysis_record` create/return | 2 tests |

**Effort**: High (5 tests, ~200 lines)
**Lines covered**: ~100

### P3.3 — `store/eventbus.rs` event handlers (192 → ~42 missed)

| Handler | Tests to add |
|---------|--------------|
| `handle_event` OpsRepoUpdated → creates release | 1 test |
| `handle_event` DeployRequested → commits to ops repo | 1 test |
| `handle_event` RollbackRequested → reverts commit | 1 test |
| `handle_event` FlagsRegistered → creates flags | 1 test |
| `handle_event` AlertFired → dispatches notification | 1 test |
| `handle_event` DevImageBuilt → updates agent image | 1 test |

**Effort**: High (6 tests, ~250 lines)
**Lines covered**: ~150

### P3.4 — `agent/service.rs` session lifecycle (376 → ~276 missed)

| Handler | Tests to add |
|---------|--------------|
| `create_session` inserts DB row | 1 test |
| `create_session` creates ephemeral identity | 1 test |
| `create_session` computes spawn depth | 1 test |
| `cleanup_session` deletes identity | 1 test |

**Effort**: High (4 tests, ~200 lines)
**Lines covered**: ~100

---

## Phase 4: Deferred / Out of Scope

| File | Lines Missed | Reason |
|------|-------------|--------|
| `observe/tracing_layer.rs` (Layer impl) | ~106 | Requires tracing subscriber setup; Phase 1 covers pure functions |
| `agent/create_app.rs` (tool loop) | ~384 | Requires mock CLI subprocess; covered by LLM test tier |
| `agent/llm_validate.rs` (test functions) | ~351 | LLM test tier only (real Claude API) |
| `onboarding/claude_auth.rs` | ~179 | Requires PTY/process spawning; test error paths only |
| `git/ssh_server.rs` | ~192 | SSH protocol hard to test; existing unit tests cover parsing |
| `store/pool.rs` | 3 | Trivial re-export |
| `registry/gc.rs` | 25 | Background GC loop; test via E2E |

---

## Implementation Order Summary

| Order | Phase | Module | Type | Est. Lines | Effort |
|-------|-------|--------|------|-----------|--------|
| 1 | P1.1 | pipeline/executor.rs | Unit | 250 | Medium |
| 2 | P1.2 | observe/tracing_layer.rs | Unit | 80 | Medium |
| 3 | P1.3 | agent/llm_validate.rs | Unit | 30 | Low |
| 4 | P1.4 | store/eventbus.rs | Unit | 30 | Low |
| 5 | P1.5 | agent/create_app.rs | Unit | 15 | Low |
| 6 | P1.6 | deployer/reconciler.rs | Unit | 15 | Low |
| 7 | P1.7 | error.rs | Unit | 15 | Low |
| 8 | P1.8 | deployer/analysis.rs | Unit | 5 | Minimal |
| 9 | P2.3 | health/checks.rs | Unit+Int | 150 | Medium |
| 10 | P2.1 | api/onboarding.rs | Integration | 200 | High |
| 11 | P2.2 | api/merge_requests.rs | Integration | 200 | High |
| 12 | P2.4 | api/admin.rs | Integration | 80 | Medium |
| 13 | P2.5 | api/passkeys.rs | Integration | 60 | Medium |
| 14 | P2.6 | api/commands.rs | Integration | 40 | Low |
| 15 | P2.7 | pipeline/trigger.rs | Integration | 150 | High |
| 16 | P3.1 | deployer/reconciler.rs | Integration | 300 | Very High |
| 17 | P3.3 | store/eventbus.rs | Integration | 150 | High |
| 18 | P3.2 | deployer/analysis.rs | Integration | 100 | High |
| 19 | P3.4 | agent/service.rs | Integration | 100 | High |

---

## Expected Coverage Impact

| Phase | Lines Covered | Cumulative Estimate |
|-------|--------------|---------------------|
| Baseline | — | 77.65% |
| Phase 1 (Unit) | ~440 | ~79.5% |
| Phase 2 (Integration) | ~1,080 | ~84.0% |
| Phase 3 (Background) | ~750 | ~87.0% |

**Conservative target**: 85% after Phases 1+2.
**Stretch target**: 87%+ after all three phases.

---

## Key Principles

1. **Unit tests first** — Phases 1.1-1.8 run with `just test-unit` (~1s), no cluster needed
2. **Test pure functions exhaustively** — `build_pod_spec`, `check_container_statuses`, `mark_transitive_dependents_skipped` are deterministic with high bug potential
3. **Integration tests follow existing patterns** — `helpers::test_state()` + `helpers::test_router()` + direct DB inserts
4. **Never mock kube client** — always use real Kind cluster
5. **Run targeted tests during dev** — `cargo nextest run --lib -E 'test(name)'` for unit, `just test-bin module test_name` for integration
6. **Run `just ci-full` once at the end** before declaring each phase complete
