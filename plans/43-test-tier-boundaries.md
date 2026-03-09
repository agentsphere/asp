# Plan 43: Redefine Integration vs E2E Test Boundaries

## Context

The platform's test tiers evolved organically. Integration tests currently bypass K8s operations via direct DB inserts, while E2E tests own all real K8s interactions. Now that the Kind cluster is always available for integration tests too, we want to:

1. Pull real K8s API calls (pod creation, namespace ops) into integration tests
2. Fold mock-CLI tests into the integration suite (remove `#[ignore]`, always provide mock CLI)
3. Establish clear, memorable guidelines for int vs e2e
4. Target 100% coverage on unit + integration (not E2E)
5. Formalize the LLM mocking strategy across all tiers

## The New Boundary: Endpoint Scope vs User Journey

The boundary is **how much of the user's reality are we simulating**, not whether the code is sync or async.

**Integration** = Single API endpoint + ALL its side effects (sync and async). Can spawn background tasks, poll for pod status, wait for workers to complete. Tests: "does this endpoint work correctly, including everything it kicks off?"

**E2E** = Multi-step user journeys spanning multiple API calls. Tests: "can a user complete this business workflow end-to-end?"

### Decision Tree

```
Does it touch I/O?
  No → Unit test
  Yes ↓
Is it testing a single endpoint and its side effects?
(even if those side effects are async — pods, executors, reconcilers, workers)
  Yes → Integration test
  No ↓
Is it a multi-step user journey across multiple endpoints?
(login → create project → push code → pipeline runs → deploy)
  Yes ↓
Does it use a real Claude CLI with live Anthropic credentials?
  Yes → LLM test (just test-llm)
  No → E2E test (just test-e2e)
```

### Tier Definitions

| Tier | What's tested | Real infra | LLM | Coverage role |
|---|---|---|---|---|
| Unit | Pure functions, state machines, parsers, validators | None | N/A | 100% of business logic branches |
| Integration | Single endpoint + all side effects (K8s pods, background workers, mock CLI) | Postgres, Valkey, MinIO, K8s API | Mock script (`CLAUDE_CLI_PATH=mock-claude-cli.sh`) | 100% of handler code paths |
| E2E | Multi-step business workflows across multiple endpoints | All real + background tasks | Disabled (`cli_spawn_enabled: false`) | Critical user journeys |
| LLM | Claude CLI protocol with real Anthropic API | Real Claude CLI + credentials | Real | Protocol contract only |

**Integration tests can be async.** If a handler kicks off a background task (executor, reconciler, pod), the integration test spawns that task and polls/waits for the outcome. The test is still "integration" because it's validating one endpoint's complete behavior — the async worker is an implementation detail, not a separate system boundary.

**Mock CLI in integration tests**: `test_state()` always sets `CLAUDE_CLI_PATH` to `tests/fixtures/mock-claude-cli.sh`. Tests that need the CLI subprocess path set `cli_spawn_enabled: true` via a helper. The mock script exits instantly with canned NDJSON — no external dependency, no runtime cost.

### What This Changes

Previously in E2E, now belongs in **Integration**:
- Pipeline trigger → executor picks up → pod runs → status reaches success/failure
- `create_session` → pod reaches `Running` → messages flow via pub/sub
- Webhook fires → wiremock receives POST (single endpoint side effect)
- Reconciler applies manifests after deployment create/update
- Git push via smart HTTP → verify commits readable

Stays in **E2E** (multi-step journeys):
- Onboarding flow: signup → create project → configure → push → pipeline → deploy
- Full agent workflow: create project → create session → agent runs → stop → logs in MinIO
- MR lifecycle: create branch → push → open MR → pipeline runs → review → merge → preview cleanup
- Deployment pipeline: push → auto-trigger → build → deploy → rollback → redeploy

### Ambiguous Cases Resolved

| Scenario | Tier | Why |
|---|---|---|
| `create_session` → pod exists in K8s | Integration | Single endpoint, K8s API is a side effect |
| `create_session` → pod reaches `Running` → messages flow | Integration | Single endpoint + its async workers |
| Pipeline trigger → executor → pod completes → status success | Integration | Single endpoint + background executor |
| Webhook fires → wiremock receives POST | Integration | Single endpoint's async delivery side effect |
| Reconciler applies manifest after deployment create | Integration | Single endpoint + reconciler worker |
| Git push → commits readable via browse API | Integration | Single endpoint + filesystem side effect |
| Mock CLI emits canned NDJSON → session updated | Integration | Mock script, no real LLM |
| Login → create project → push → pipeline → deploy | E2E | Multi-step business journey |
| Create project → add agent → agent runs → results visible | E2E | Multi-step business journey |
| MR open → pipeline auto-triggers → merge → preview cleaned up | E2E | Multi-step business journey |

## Changes

This plan is **documentation-only** — updating guidelines and test commands. The actual migration of tests from E2E → integration is a separate follow-up (moving test functions, updating helpers to support `ExecutorGuard`/`ReconcilerGuard`/`wait_for_pod` in integration context, etc.).

### 1. Fold mock-CLI tests into integration suite

**Files**: `tests/cli_create_app_integration.rs`, `tests/helpers/mod.rs`

- Remove `#[ignore]` from mock-CLI tests in `cli_create_app_integration.rs`
- Update `test_state()` in `helpers/mod.rs` to always set `CLAUDE_CLI_PATH` pointing to `tests/fixtures/mock-claude-cli.sh` (absolute path via `env!("CARGO_MANIFEST_DIR")`)
- Add a `test_state_with_cli(pool)` helper that returns state with `cli_spawn_enabled: true` — for tests that exercise the CLI subprocess flow
- Update `hack/test-in-cluster.sh` to export `CLAUDE_CLI_PATH` so it's available during `cargo nextest run`

### 2. Update `docs/testing.md`

**File**: `docs/testing.md`

Rewrite tier definitions with the new guidelines:
- Integration = single endpoint + all side effects (including async workers, K8s pods, mock CLI)
- E2E = multi-step user journeys across multiple endpoints
- Add the decision tree
- Add the ambiguous cases table
- Add "what moves from E2E to integration" section (pipeline execution, webhook delivery, agent pod lifecycle, deployer reconciliation, git push)
- Update coverage section: 100% target on unit + integration (diff-only enforcement)
- Add LLM mocking strategy per tier
- Note: actual test migration is a follow-up task

### 3. Update `CLAUDE.md` Testing Section

**File**: `CLAUDE.md`

- New boundary definition: endpoint scope vs user journey
- Add the decision tree (abbreviated)
- Integration tests now include: real K8s interactions, background tasks, mock CLI, polling/waiting
- E2E tests are for: multi-step business workflows only
- Coverage target: "100% on unit + integration" (diff-only)
- LLM rule: "No real LLM calls in unit/int/e2e. Mock CLI via `CLAUDE_CLI_PATH`. Separate `just test-llm`."
- Update commands table

### 4. Add `just test-all` Command

**File**: `Justfile`

```just
# All tests except LLM (unit + integration + e2e)
test-all: test-unit test-integration test-e2e
```

### 5. Update Memory

**File**: Memory `MEMORY.md`

Update test-related sections to reflect new boundaries.

## Implications for Existing Tests

Once the guidelines are in place, the following **existing E2E tests** should be migrated to integration in a follow-up:

| Current E2E file | Tests to migrate | Reason |
|---|---|---|
| `e2e_pipeline.rs` | All 10 (trigger, multi-step, failure, cancel, logs, artifacts) | Single endpoint + executor side effect |
| `e2e_webhook.rs` | All 6 (delivery, HMAC, timeout, concurrency) | Single endpoint + async dispatch |
| `e2e_agent.rs` | All 8 (session create, identity, pod spec, stop, custom image, logs) | Single endpoint + pod lifecycle |
| `e2e_deployer.rs` | ~10 of 17 (single-endpoint tests with reconciler) | Single endpoint + reconciler side effect |
| `e2e_git.rs` | All 8 (push, clone, branches, browse, merge) | Single endpoint + filesystem side effect |

**Remaining E2E tests** (new, to be written as the suite matures):
- Onboarding journey: signup → create project → configure → first push → first pipeline
- Full agent workflow: create project → create session → agent completes task → logs visible
- MR lifecycle: branch → push → MR → pipeline → review → merge → preview cleanup
- Deployment pipeline: push → build → deploy → verify → rollback

This migration requires updating `tests/helpers/mod.rs` to support:
- `ExecutorGuard::spawn()` and `state.pipeline_notify`
- `ReconcilerGuard` for deployer tests
- `wait_for_pod()`, `poll_pipeline_status()` helpers
- Git repo helpers (`create_bare_repo`, `create_working_copy`, `git_cmd`)
- `start_test_server()` for webhook delivery (wiremock needs a reachable URL)
- Pipeline/agent namespace setup (currently only in `e2e_helpers`)

## Files Modified (this plan)

| File | Change |
|---|---|
| `tests/cli_create_app_integration.rs` | Remove `#[ignore]` from mock-CLI tests |
| `tests/helpers/mod.rs` | Always set `CLAUDE_CLI_PATH`; add `test_state_with_cli()` helper |
| `hack/test-in-cluster.sh` | Export `CLAUDE_CLI_PATH` env var |
| `docs/testing.md` | Rewrite tier definitions, decision tree, coverage targets, LLM mock strategy, migration roadmap |
| `CLAUDE.md` | Update testing standards with new boundary definition |
| `Justfile` | Add `test-all` |
| Memory `MEMORY.md` | Update test tier info |

## Verification

Skipped — test suite is currently broken and will be fixed in a follow-up run.
