# Skill: Test Quality Audit — Coverage Gaps, Correctness & Tier Classification

**Description:** Orchestrates 5 parallel AI agents that audit the test suite: coverage gaps, assertion quality, test-tier classification, mock fidelity, edge case coverage, and flaky test patterns. The core question: *"Do these tests actually catch the bugs they're supposed to catch?"*

**When to use:** After large refactoring, when tests pass but bugs still ship, when test suite is slow, when coverage numbers don't reflect actual confidence, or when test infrastructure changes.

---

## Orchestrator Instructions

You are the **Test Auditor**. Your job is to:

1. Inventory all test files and understand the test architecture
2. Launch 5 parallel agents analyzing different test quality dimensions
3. Synthesize findings into a prioritized report
4. Produce a persistent `plans/test-audit-<date>.md` report

### Severity Levels

| Severity | Meaning | Action |
|---|---|---|
| **CRITICAL** | Test passes but code is wrong (false positive), or critical path untested | Fix immediately |
| **HIGH** | Weak assertions (checks status but not body), wrong tier, missing auth test | Fix before release |
| **MEDIUM** | Missing edge case, inconsistent pattern, suboptimal mock | Fix when touching the area |
| **LOW** | Naming nit, redundant test, minor style issue | Fix only if trivial |

---

## Phase 0: Inventory

```bash
# Test file inventory
echo "=== Unit tests (in src/) ==="
grep -rl '#\[cfg(test)\]' src/ --include='*.rs' | sort

echo "=== Integration tests ==="
ls tests/*_integration.rs 2>/dev/null

echo "=== E2E tests ==="
ls tests/e2e_*.rs 2>/dev/null

echo "=== Test helpers ==="
ls tests/helpers/ tests/e2e_helpers/ 2>/dev/null

echo "=== Test fixtures ==="
ls tests/fixtures/ 2>/dev/null

echo "=== Test counts ==="
grep -rn '#\[test\]' src/ --include='*.rs' | wc -l
grep -rn '#\[sqlx::test' tests/ --include='*.rs' | wc -l
grep -rn '#\[test\]' tests/ --include='*.rs' | wc -l

echo "=== Ignored tests ==="
grep -rn '#\[ignore' tests/ src/ --include='*.rs' | head -20
```

---

## Phase 1: Parallel Test Audits

Launch **all 5 agents concurrently**.

---

### Agent 1: Coverage Gap Analysis

**Scope:** All `src/` code cross-referenced with all test files

**Identify untested code paths:**

_Module coverage:_
- [ ] Which `src/` modules have NO tests (neither unit nor integration)?
- [ ] Which modules have unit tests but no integration tests?
- [ ] Which API handlers have no corresponding integration test?

_Critical path coverage:_
- [ ] Auth: login, token validation, session creation — all tested?
- [ ] RBAC: permission checking, delegation, cache invalidation — all tested?
- [ ] Password: hash, verify, dummy_hash timing safety — all tested?
- [ ] Encryption: encrypt, decrypt, round-trip — all tested?
- [ ] Pipeline: trigger, execute, status transitions — all tested?
- [ ] Deployer: reconcile, apply, rollback — all tested?
- [ ] Agent: session create, pod launch, cleanup — all tested?

_Error path coverage:_
- [ ] For each handler: is the 4xx path tested (bad input, unauthorized, not found)?
- [ ] For each handler: is the 5xx path tested (DB down, K8s error)?
- [ ] Validation: are boundary values tested (empty string, max length, null bytes)?
- [ ] Rate limiting: is the rate-limited path tested?

_State machine coverage:_
- [ ] Pipeline status: is every valid transition tested? Every invalid transition rejected?
- [ ] Deployment status: same check
- [ ] Session status: same check

_Missing test categories:_
- [ ] Concurrency tests: are race conditions tested (two concurrent requests)?
- [ ] Cleanup tests: does session/pipeline termination clean up all resources?
- [ ] Cascade tests: does deleting a parent clean up children?
- [ ] Soft-delete tests: are soft-deleted items truly invisible in list/get?

**Output:** Numbered findings with module/handler, what's untested, and suggested test.

---

### Agent 2: Assertion Quality & False Positives

**Scope:** All test files (`tests/*.rs`, `src/**/tests.rs`)

**Read ALL tests and check assertion strength:**

_Weak assertions:_
- [ ] Tests that only check `status == 200` without verifying response body
- [ ] Tests that only check `status != 500` (passes for wrong status codes)
- [ ] Tests that check `body.contains("something")` instead of parsing JSON and checking fields
- [ ] Tests that use `assert!(result.is_ok())` without checking the Ok value

_Missing assertions:_
- [ ] Tests that create/update but don't verify the change persisted (read-back check)
- [ ] Tests that trigger side effects but don't verify them (audit log, webhook, notification)
- [ ] Tests that don't verify error message content (just status code)
- [ ] Tests that don't verify response headers where relevant (content-type, cache headers)

_False positive risk:_
- [ ] Tests that would pass even if the feature were completely broken (vacuous tests)
- [ ] Tests where the setup matches the assertion (testing the test, not the code)
- [ ] Tests that rely on default/empty state as "success" (absence of failure ≠ success)
- [ ] Tests with broad error matching (`assert!(err.to_string().contains(""))`) that never fails

_Assertion patterns:_
- [ ] Consistent use of `assert_eq!` vs `assert!` (prefer `assert_eq!` for better error messages)
- [ ] Floating point comparisons (should use epsilon, not exact)
- [ ] UUID/timestamp comparisons (should check format/presence, not exact values)

**Output:** Numbered findings with test file:line, weak assertion, and stronger alternative.

---

### Agent 3: Test Tier Classification

**Scope:** All tests, cross-referenced with the test tier decision tree from CLAUDE.md

The test tier boundaries are:
- **Unit** = Pure functions, no I/O
- **Integration** = Single API endpoint + ALL its side effects
- **E2E** = Multi-step user journeys spanning multiple API calls

**Check every test is in the right tier:**

_Misclassified tests:_
- [ ] Tests in `src/` (unit tier) that require I/O (DB, Valkey, K8s, network)
- [ ] Tests in `tests/*_integration.rs` that span multiple endpoints (should be E2E)
- [ ] Tests in `tests/e2e_*.rs` that only test a single endpoint (should be integration)
- [ ] Tests that require a cluster but are marked as unit tests

_Test infrastructure misuse:_
- [ ] Integration tests not using `#[sqlx::test]` (missing DB isolation)
- [ ] Tests that call `helpers::test_state()` but are in the wrong tier
- [ ] Tests that directly manipulate DB state instead of using API calls (in E2E tier)
- [ ] E2E tests that use `helpers` instead of `e2e_helpers`

_Missing tier for coverage:_
- [ ] Features tested only at E2E level (should also have integration tests for speed)
- [ ] Features tested only at unit level (should have integration tests for wiring)
- [ ] Integration tests that should be unit tests (pure function tested through API)

**Output:** Numbered findings with test name, current tier, correct tier, and migration steps.

---

### Agent 4: Mock Fidelity & Test Infrastructure

**Scope:** `tests/helpers/mod.rs`, `tests/e2e_helpers/mod.rs`, `tests/fixtures/`, mock setup in test files

**Verify test infrastructure quality:**

_Mock CLI:_
- [ ] `tests/fixtures/claude-mock/claude` — does its output match real Claude CLI format?
- [ ] Mock NDJSON events: do they cover all `ProgressKind` variants?
- [ ] Mock errors: are error scenarios covered?

_Test state builders:_
- [ ] `test_state()` — does it match production `AppState` construction? Any missing fields?
- [ ] `test_router()` — does it match production router construction? Any missing routes?
- [ ] `e2e_state()` — same checks
- [ ] Are there test-only code paths that differ from production? (Test-specific bypasses)

_Valkey isolation:_
- [ ] Tests never call FLUSHDB (documented rule) — verify
- [ ] All Valkey keys UUID-scoped (no collision between parallel tests)
- [ ] Rate limit bypass for admin token — documented and consistent

_Database isolation:_
- [ ] `#[sqlx::test]` provides isolated DB per test — verify all integration tests use it
- [ ] Dynamic queries in tests (`sqlx::query()` not `sqlx::query!()`) — verify
- [ ] No shared mutable state between tests

_K8s test resources:_
- [ ] Test RBAC (`hack/test-manifests/rbac.yaml`) matches production Helm RBAC
- [ ] Test namespaces cleaned up after tests
- [ ] Pod creation in tests uses correct namespace

_Test data builders:_
- [ ] `create_user()`, `assign_role()` — do they reflect actual API behavior?
- [ ] Are there hardcoded IDs/names that could collide?
- [ ] Are test data builders kept up-to-date when API changes?

**Output:** Numbered findings with infrastructure component, fidelity gap, and fix.

---

### Agent 5: Edge Cases, Flakiness & Test Maintenance

**Scope:** All test files

**Scan for problems:**

_Missing edge cases:_
- [ ] Empty inputs: empty string, empty array, null/None
- [ ] Boundary values: max length strings, max pagination limit, zero offset
- [ ] Unicode: non-ASCII names, emoji in descriptions, RTL text
- [ ] Special characters: SQL metacharacters, HTML in text fields, null bytes
- [ ] Concurrent operations: two users editing same issue, parallel pipeline triggers
- [ ] Timing: expired sessions, expired tokens, rate limit window boundary

_Flaky test indicators:_
- [ ] Tests with `tokio::time::sleep()` — timing-dependent (could flake)
- [ ] Tests that depend on ordering of concurrent operations
- [ ] Tests that poll with fixed iteration count (might not be enough on slow CI)
- [ ] Tests that depend on external network (image pulls, DNS resolution)
- [ ] Tests with non-deterministic UUIDs in assertions (comparing against `Uuid::new_v4()`)

_Test maintenance:_
- [ ] Commented-out tests — why? Still needed?
- [ ] `#[ignore]` tests — documented reason? Still valid?
- [ ] Tests with TODO comments — what's blocked?
- [ ] Duplicate tests (testing same thing in different files)
- [ ] Tests that are order-dependent (pass individually, fail together)

_Test naming:_
- [ ] Descriptive names that explain what's being tested and expected behavior
- [ ] Consistent naming convention across test files
- [ ] Names that would make sense in a CI failure report

**Output:** Numbered findings with test name, issue, and fix.

---

## Phase 2: Synthesis

Deduplicate, prioritize, categorize:
- **False positives** — tests that pass but shouldn't (or would pass with broken code)
- **Coverage gaps** — critical paths without tests
- **Tier misclassification** — tests in wrong tier affecting CI speed/reliability
- **Weak assertions** — tests that check too little
- **Missing edge cases** — boundaries not tested
- **Flaky risks** — timing-dependent or order-dependent tests

Number findings T1, T2, T3... (T for Test)

---

## Phase 3: Write Report

Persist as `plans/test-audit-<YYYY-MM-DD>.md`.

Include:
- Test inventory table (module → unit count → integration count → E2E count)
- Coverage gap matrix (feature → tested tiers → missing tiers)
- Weak assertion list (test → current assertion → stronger assertion)
- Tier classification corrections
- Flaky test risk list

---

## Phase 4: Summary to User

1. Test suite health (one sentence)
2. Finding counts
3. Top 3 coverage gaps (critical untested paths)
4. Top 3 false positive risks
5. Tier misclassification count
6. Path to report
