# Skill: Improve Coverage — Targeted Test Writing for Uncovered Code

**Description:** Reads a coverage report, picks the highest-impact uncovered module (or one the user specifies), reads the source to understand the gaps, writes tests, and verifies them. Designed for iterative use: run it, let it fix one area, run it again.

**Usage:**
```
/improve-coverage                          # auto-pick highest-impact module
/improve-coverage pipeline/executor        # target a specific module
/improve-coverage --checkpoint             # run just cov-total and show progress
```

Refer to `CLAUDE.md` for all coding patterns, conventions, and architecture rules.

---

## Step 0: Parse Coverage Data

### 0.1 Find the latest coverage report

Look for coverage data in this priority order:
1. User pasted coverage table in the conversation
2. Latest `test-*-coverage.txt` file in the project root
3. If neither exists, run `just cov-summary` (unit-only, fast ~5s) to get a baseline

### 0.2 Parse into a ranked list

From the coverage data, build a priority table sorted by **missed lines descending**:

| Module | Line Coverage | Missed Lines | Type |
|---|---|---|---|
| `pipeline/executor.rs` | 47% | 1,748 | Integration |
| `deployer/reconciler.rs` | 28% | 799 | Integration |
| ... | | | |

### 0.3 Classify each module

For each module, determine the test type needed:

| Classification | Criteria | Test approach |
|---|---|---|
| **Unit-testable** | Pure logic, parsers, validators, type conversions, state machines | `#[cfg(test)] mod tests` in the source file, verify with `just test-unit` |
| **Integration-needed** | DB queries, API handlers, K8s operations, auth flows | `tests/*_integration.rs`, verify with `just test-bin <binary> <filter>` |
| **Hard-to-test** | Background loops, main.rs wiring, real LLM calls | Document as exception, skip unless user explicitly requests |

### 0.4 Select target

If the user specified a module: use that.
Otherwise: pick the module with the **most missed lines** that isn't classified as hard-to-test.

Announce the target to the user:
```
Target: src/pipeline/executor.rs (1,748 missed lines, 47% coverage)
Classification: Integration — needs tests in tests/pipeline_integration.rs
```

---

## Step 1: Understand the Gap

### 1.1 Read the source file

Read the **entire** target source file. For files >500 lines, read in chunks.

### 1.2 Read existing tests

Find and read all existing tests for this module:
- Unit tests: `#[cfg(test)] mod tests` block in the source file
- Integration tests: `tests/<module>_integration.rs` (find the right file)
- E2E tests: grep for the module name in `tests/e2e_*.rs`

### 1.3 Identify uncovered code

Cross-reference the source with existing tests. Build a list of **untested functions/branches**:

```
Uncovered in src/pipeline/executor.rs:
1. run_pipeline_step() lines 145-200 — error handling when pod fails to start
2. cleanup_step_resources() lines 310-350 — entire function untested
3. handle_step_timeout() lines 400-430 — timeout branch never exercised
```

### 1.4 Prioritize within the module

Order by impact:
1. **Entire untested functions** — biggest coverage gain per test
2. **Error/failure branches** — important for correctness
3. **Edge cases in tested functions** — smaller coverage gain but catches bugs
4. **Trivial display/conversion impls** — lowest priority

### 1.5 Plan the tests

For each gap, decide:
- Test name (descriptive: `executor_returns_failure_when_pod_oom_killed`)
- Test tier (unit or integration)
- What setup is needed (state builder, mock data, K8s resources)
- What to assert (specific values, not just `is_ok()`)

Announce the plan to the user (keep it brief — 5-10 tests max per run):
```
Writing 6 tests for pipeline/executor.rs:
1. [integration] executor_returns_failure_when_pod_oom_killed
2. [integration] executor_cleans_up_resources_on_failure
3. [unit] step_timeout_config_defaults_to_600s
...
```

---

## Step 2: Write Tests

### 2.1 Follow project test patterns strictly

**Unit tests** go in the source file:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn my_test() { ... }
}
```

**Integration tests** go in `tests/<module>_integration.rs`:
```rust
#[sqlx::test(migrations = "./migrations")]
async fn my_test(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());
    // ...
}
```

### 2.2 Critical rules (violating these causes test failures)

- **`helpers::test_state(pool).await`** for state — never `admin_login()` (rate limit collision)
- **`sqlx::query()`** (dynamic) in test files — never `sqlx::query!()` (needs offline cache)
- **Never FLUSHDB** — all Valkey keys are UUID-scoped
- **Pipeline tests**: spawn executor + `state.pipeline_notify.notify_one()`
- **Webhook URLs**: insert directly into DB (SSRF blocks localhost)
- **E2E git repos**: under `/tmp/platform-e2e/`
- **Mock CLI**: use `test_state_with_cli(pool, true)` for agent/session tests
- **No `#[ignore]`** on integration tests — only E2E tests get `#[ignore]`

### 2.3 Assertion quality

Every test must have **strong assertions**:
- Assert specific status codes AND response body fields
- For mutations: read back the resource and verify it changed
- For errors: assert the error message or error code, not just the status
- For state machines: assert both the new state and that invalid transitions are rejected

### 2.4 Write incrementally

Write tests in small batches (2-3 at a time), verify each batch compiles before moving on. This prevents wasted work if there's a pattern issue.

---

## Step 3: Verify Tests Pass

### 3.1 Compile check first

```bash
cargo check --tests 2>&1 | tail -20
```

If compilation fails, fix the tests before running them.

### 3.2 Run targeted tests only

**Unit tests** (if you wrote unit tests):
```bash
cargo nextest run --lib -E 'test(my_test_name)' 2>&1 | tail -20
```

**Integration tests** (fast, targeted):
```bash
just test-bin <integration_binary> <test_filter>
```

For example:
```bash
just test-bin pipeline_integration executor_returns_failure
```

### 3.3 Fix failures

If tests fail:
1. Read the error output carefully
2. Determine if the test is wrong or the code has a bug
3. If the test is wrong: fix the test
4. If the code has a bug: fix the code AND note it for the user
5. Re-run the targeted test

### 3.4 Do NOT run full coverage yet

`just cov-total` takes ~3 min. Only run it when:
- The user explicitly asks for a checkpoint (`/improve-coverage --checkpoint`)
- You've completed 3+ modules and want to verify progress
- You're done for the session

---

## Step 4: Report & Next Steps

### 4.1 Report what was done

```
Done: src/pipeline/executor.rs
  Added: 6 integration tests, 2 unit tests
  New functions covered: run_pipeline_step error path, cleanup_step_resources, handle_step_timeout
  Estimated coverage improvement: ~400 lines (47% → ~70%)

  Next highest-impact: src/deployer/reconciler.rs (799 missed lines)
  Run /improve-coverage again to continue, or /improve-coverage --checkpoint to measure.
```

### 4.2 Track progress

If a plan file exists at `plans/improve-test-coverage.md`, update it with:
- Which module was worked on
- How many tests were added
- Approximate coverage change

If no plan file exists, create one on first run:

```markdown
# Test Coverage Improvement Tracker

**Target:** 100% line coverage on unit + integration
**Started:** <date>

## Progress

| Module | Before | Tests Added | Status |
|---|---|---|---|
| `pipeline/executor.rs` | 47% (1,748 missed) | 8 | Done |
```

---

## Checkpoint Mode (`--checkpoint`)

When the user passes `--checkpoint`:

1. Run `just cov-total` (full coverage measurement)
2. Read the resulting `test-*-report.txt` to verify all tests pass
3. Read the coverage output
4. Compare against the tracker in `plans/improve-test-coverage.md`
5. Update the tracker with actual numbers
6. Show a summary:

```
Coverage checkpoint:
  Before: 80.16% (8,082 missed lines)
  After:  84.5% (6,300 missed lines)
  Δ: +4.34% (1,782 lines covered)

  Remaining top gaps:
  1. deployer/reconciler.rs — 750 missed lines
  2. agent/service.rs — 350 missed lines
  ...
```

---

## Module-Specific Guidance

Some modules need special test setup. Check this table before writing tests:

| Module | Special setup | Notes |
|---|---|---|
| `pipeline/executor.rs` | Spawn executor task, use `pipeline_notify` | Tests must wait for pod completion |
| `deployer/reconciler.rs` | Real K8s client, create test deployments | Use `cleanup_k8s()` in setup |
| `agent/service.rs` | `test_state_with_cli(pool, true)` for mock CLI | Agent sessions need mock CLI responses |
| `agent/create_app.rs` | Mock CLI + project setup | Create app flow spans multiple services |
| `agent/llm_validate.rs` | Unit-testable validation logic + mock responses | Separate pure validation from API calls |
| `git/ssh_server.rs` | Real SSH server on random port | Use `start_test_server()` pattern |
| `health/checks.rs` | State with degraded services | Test each health check independently |
| `store/eventbus.rs` | Valkey pub/sub with `pool.next().clone_new()` | Subscriber needs dedicated connection |
| `observe/mod.rs` | Background task spawning | Test init + shutdown lifecycle |
| `api/merge_requests.rs` | Project + branches + MR setup | Heavy setup, reuse across tests |
| `api/onboarding.rs` | Fresh state, no existing data | Onboarding checks for empty DB |
| `registry/*` | MinIO bucket + manifest data | Use existing registry test patterns |

---

## What NOT to Do

- **Don't write tests for `main.rs` wiring** — covered by E2E, not worth unit/integration tests
- **Don't write tests for `proto.rs` or `ui.rs`** — generated/embedded code
- **Don't mock the database** — use real Postgres via `#[sqlx::test]`
- **Don't mock K8s** — use real Kind cluster
- **Don't add `#[ignore]` to skip hard tests** — either write the test properly or skip the function
- **Don't chase 100% on error Display impls** — low value
- **Don't run `just cov-total` after every file** — it's 3 min each time
- **Don't write tests that only assert `is_ok()`** — always check specific values
- **Don't write redundant tests** — if an E2E test already covers the path, don't duplicate at integration level unless it adds value (faster feedback, specific edge case)

---

## Principles

- **Highest impact first** — 1 test covering 50 uncovered lines beats 5 tests covering 5 lines each
- **Real infra, no mocks** — match production behavior as closely as possible
- **Fast feedback** — use targeted test runs, not full suite
- **Incremental** — write 2-3 tests, verify, repeat. Don't write 20 tests and hope they all compile.
- **Strong assertions** — every test should fail if the code breaks. A test that always passes is worse than no test.
- **Read before write** — understand the code before testing it. Tests written without understanding the code test the wrong things.
