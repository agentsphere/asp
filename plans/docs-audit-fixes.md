# Plan: Docs Audit Fixes

## Context

The documentation audit (`plans/docs-audit-2026-03-25.md`) identified 67 findings across all documentation surfaces. Key themes: stale counts everywhere (LOC, modules, migrations, tests), incomplete CLAUDE.md (AppState, commands, env vars), MCP contract mismatches, and template issues.

Additionally, test infrastructure improvements (RUN_ID-suffixed output files, `TEST_LOG_FILE` for structured JSON logs with `test.name`) were reverted and need re-applying. Coverage defaults need updating to target unit+integration (not E2E) by default.

This plan addresses all findings in 3 PRs plus a test infrastructure fix.

## Design Principles

- **Verify before fixing** — every number change confirmed against live codebase
- **No aspirational docs** — only document what actually exists
- **Batch stale counts** — a single pass through each file fixes all numbers at once

---

## PR 1: CLAUDE.md + docs/ Stale Counts & Content

Addresses: **D1-D4, D9-D19, D27-D35, D46, D49-D55** (8 CRITICAL + 12 HIGH + 11 MEDIUM + 7 LOW)

This is the highest-impact PR: fixes all stale numbers and content in the main coding guidelines and docs folder.

- [ ] Types & errors defined (N/A)
- [ ] Migration applied (N/A)
- [ ] Tests written (N/A)
- [ ] Implementation complete
- [ ] Quality gate passed

### CLAUDE.md Fixes

| Finding | Line | Current | Correct | Verified |
|---|---|---|---|---|
| D1 | ~48 | "11 modules" | "15 modules" | `find src -name mod.rs -maxdepth 2` → 15 |
| D2 | ~49-59 | 7 AppState fields | 13 fields | `src/store/mod.rs:19-40` |
| D3 | ~1 | "~23K LOC" | "~72K LOC" | `find src -name '*.rs' \| xargs wc -l` → 72,076 |
| D9 | ~16 | `just test` | Remove (doesn't exist) | `grep -E '^test[: ]' Justfile` → empty |
| D10 | ~35 | cov-total "unit + integration + E2E" | "unit + integration (needs cluster + DB)" | see Justfile changes below |
| D11 | ~235 | "in src/pipeline/definition.rs" | "in src/validation.rs" | verified |
| D12 | ~697 | `ExecutorGuard::spawn` | Update to actual pattern: `tokio::spawn(executor::run(state.clone()))` | no `ExecutorGuard` exists |
| D13 | ~334 | "6 MCP servers" | "7 MCP servers" + add `platform-browser.js` | `ls mcp/servers/*.js` → 7 |
| D14 | ~7-43 | 15+ recipes missing | Add: `dev-up`, `dev-down`, `dev`, `test-bin`, `test-llm`, `test-cleanup`, `build-agent-images`, `registry-login`, `docs-viewer`, `types` | verified in Justfile |
| D15 | ~447-466 | ~25 env vars | ~50 env vars | `src/config.rs` has ~50 |
| D46 | ~357 | check_email "1-254" | "3-254" | `src/validation.rs:43` enforces min=3 |
| D49 | ~40 | `just ci` description incomplete | Add: cli-lint, cli-test, mcp-test | verified in Justfile |
| D50 | ~39 | cov-summary "unit + integration" | "unit only" | verified |
| D51 | ~11 | `just run` description | Add "(with env file)" | verified |
| D52 | ~321 | UI lib: ws.ts listed | Replace with sse.ts, add onboarding.tsx, webauthn.ts | verified |
| D53 | ~318 | UI pages/components incomplete | Update lists | verified |
| D54 | ~460 | WebAuthn defaults shown as "—" | Show actual defaults (localhost) | verified |
| D55 | ~180 | `has_permission()` example | Change to `has_permission_scoped()` | verified |

### AppState struct update (D2)

Replace the 7-field example with the actual 13-field struct from `src/store/mod.rs`:

```rust
pub struct AppState {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub minio: opendal::Operator,
    pub kube: kube::Client,
    pub config: Arc<Config>,
    pub webauthn: Arc<webauthn_rs::prelude::Webauthn>,
    pub pipeline_notify: Arc<tokio::sync::Notify>,
    pub deploy_notify: Arc<tokio::sync::Notify>,
    pub secret_requests: SecretRequests,
    pub cli_sessions: CliSessionManager,
    pub health: Arc<std::sync::RwLock<HealthSnapshot>>,
    pub task_registry: Arc<TaskRegistry>,
    pub cli_auth_manager: Arc<CliAuthManager>,
}
```

### docs/ Folder Fixes

| File | Findings | Changes |
|---|---|---|
| `docs/architecture.md` | D3, D4, D17, D27, D28 | LOC 23K→72K, migrations 24→64, modules 11→15, API files 14→30, "replacing 8+" keep (6 named + 2 implied) |
| `docs/testing.md` | D16, D35 | Integration files 26→52, E2E files 5→9, add CI recipe details |
| `docs/feature-inventory.md` | D18, D33, D34 | Fix all file counts: api 23→30, git 8→12, secrets 3→5, deployer 8→11, agent 21→23, store 5→6, notify 3→4 |
| `docs/arc42/01-introduction-goals.md` | D19 | Test count stale → update |
| `docs/arc42/02-constraints.md` | D3, D4, D19 | LOC 23K→72K, migrations 28→64, module count |
| `docs/arc42/03-context-scope.md` | D4, D19 | Migration count |
| `docs/arc42/04-solution-strategy.md` | D4, D19 | Migration count, module count |
| `docs/arc42/05-building-blocks.md` | D19, D30 | Module list update, remove nonexistent registry "proxy" |
| `docs/arc42/06-runtime-view.md` | D31, D32 | Fix plan reference, add Pending→Cancelled transition |
| `docs/arc42/08-crosscutting-concepts.md` | D19 | Test count |
| `docs/arc42/09-architecture-decisions.md` | D19, D29 | Module count, config field count 87→~52 |
| `docs/arc42/10-quality-requirements.md` | D19 | Test count |

### Verification
- All numbers cross-referenced with `grep`, `wc`, `find`, `ls` commands
- `just ci` still passes (no code changes)

---

## PR 2: Templates + MCP + UI + Seed Fixes

Addresses: **D5-D8, D22-D26, D36-D42, D43, D56-D59** (3 CRITICAL + 7 HIGH + 8 MEDIUM + 4 LOW)

- [ ] Types & errors defined (N/A)
- [ ] Migration applied (N/A)
- [ ] Tests written (N/A)
- [ ] Implementation complete
- [ ] Quality gate passed

### MCP Fixes (contract mismatches)

| File | Finding | Fix |
|---|---|---|
| `mcp/servers/platform-deploy.js:237` | D8: sends `weight` | Change to `traffic_weight` to match `AdjustTrafficRequest` |
| `mcp/servers/platform-deploy.js:60` | D22: strategy "blue_green" | Verify actual enum values in `src/api/deployments.rs` and fix |
| `mcp/servers/platform-deploy.js:104` | D23: sends `target_id` | Remove if API doesn't accept it |
| `mcp/servers/platform-deploy.js:66` | D24: environment required | Remove from required array |
| `mcp/servers/platform-admin.js:57` | D25: user_type "service" | Change to "service_account" (Rust enum is `ServiceAccount`) |
| `mcp/servers/platform-deploy.js` | D41: list_releases target_id filter | Remove if API ignores it |
| `mcp/servers/platform-admin.js` | D42: password required | Change to optional (only required for human) |
| `mcp/servers/platform-admin.js` | D56: list_roles limit/offset | Verify if API now accepts them (PR3 wrapped in ListResponse) |
| `mcp/servers/platform-deploy.js` | D58: create_target branch field | Remove if API has no branch field |

### Template Fixes

| File | Finding | Fix |
|---|---|---|
| `src/git/templates/CLAUDE.md` | D5: lists nonexistent files | Change "Project Structure" to "Starter Template — create these files" |
| `src/git/templates/CLAUDE.md` | D6: Dockerfile copies absent files | Add note: "Create app/ and requirements.txt before building" |
| `src/git/templates/CLAUDE.md` | D7: kubectl apply on template YAML | Add note: "Render template values before applying" or provide a `make deploy` target |
| `src/git/templates/CLAUDE.md` | D36: canary traffic steps described as fixed | Note they're configurable |
| `src/git/templates/CLAUDE.md` | D37: requirements.txt listed as existing | Mark as "to create" |
| `src/git/templates/.claude/commands/dev.md` | D38: kubectl apply template file | Same fix as D7 |
| `src/git/templates/CLAUDE.md` | D40: COMMIT_SHA/REGISTRY conditional | Note with `${REGISTRY:-}` guard |
| `seed-commands/dev.md` | D26: references deploy/production.yaml | Update to current template names |
| `seed-commands/dev.md` | D39: missing push/MR/build steps | Add steps 7-8 |

### UI Fix

| File | Finding | Fix |
|---|---|---|
| `ui/src/lib/types.ts` | D43: stale Deployment/DeploymentHistory types | Verify actual types and update (may be auto-generated via `just types`) |

### Verification
- MCP tests pass: `node --test mcp/tests/test-*.js`
- Template renders without errors
- `just ui` builds successfully

---

## PR 3: Code Cleanup + README + Infrastructure

Addresses: **D20-D21, D44-D48, D60-D67** (2 HIGH + 4 MEDIUM + 13 LOW)

- [ ] Types & errors defined (N/A)
- [ ] Migration applied (N/A)
- [ ] Tests written (N/A)
- [ ] Implementation complete
- [ ] Quality gate passed

### Dead Code Cleanup (D20-D21)

| File | Finding | Fix |
|---|---|---|
| `src/error.rs:5` | D20: `#[allow(dead_code)]` on ApiError | Remove — all variants consumed |
| `src/store/mod.rs:18` | D20: `#[allow(dead_code)]` on AppState | Remove — all fields consumed |
| `src/config.rs:5` | D20: `#[allow(dead_code)]` on Config | Remove — all fields consumed |
| `src/secrets/mod.rs:2` | D20: `#[allow(dead_code)]` on engine | Remove — used in 15+ locations |
| `src/agent/pubsub_bridge.rs:14` | D20: `#[allow(dead_code)]` on publish_event | Remove — called in 11+ locations |
| `src/agent/claude_cli/mod.rs` | D21: stale TODO + dead_code allows | Remove TODO comment and dead_code allows |

**Caution:** Only remove `#[allow(dead_code)]` on items verified as consumed. Run `cargo check` after each removal to confirm no dead_code warnings appear.

### README Fix

| File | Finding | Fix |
|---|---|---|
| `README.md` | D44: references plans/01-foundation.md etc. | Remove stale plan references |
| `README.md` | D45: test counts stale | Update or remove specific counts |

### Infrastructure Fixes

| File | Finding | Fix |
|---|---|---|
| `hack/test-in-cluster.sh:6` | D47: references "OrbStack" | Change to "Kind" |
| `docker/Dockerfile.platform-runner-bare:2-3` | D48: claims "only Node.js, git, curl, sudo" | Add "and Kaniko executor" |
| `docker/Dockerfile.dev-pod:6` | D60: k3s import command | Change to Kind |
| `docker/Dockerfile.dev-pod:65` | D61: `just test-ui-headless` | Remove or update |
| `hack/deploy-services.sh` | D62: header omits preview-proxy | Add to service list |
| `.pre-commit-config.yaml` | D67: check-yaml exclude unexplained | Add comment |

### Module Doc Comments (D64)

Add `//!` module-level doc comments to the 14 modules missing them. One line each:

```rust
//! Pipeline definition, execution, and status management.
```

### Inline Fixes (D65-D66)

| File | Finding | Fix |
|---|---|---|
| `src/auth/middleware.rs:12` | D65: "set as request extension" | Change to "FromRequestParts extractor" |
| `src/pipeline/executor.rs` | D66: 4 legacy dead functions | Remove if truly unused |

### Verification
- `cargo check` — no new warnings from dead_code removal
- `cargo clippy` — clean
- `just test-unit` — passes

---

## PR 4: Test Infrastructure — RUN_ID Output Files + Coverage Defaults

Addresses: reverted test-in-cluster.sh improvements + coverage default change.

- [x] Types & errors defined (N/A)
- [x] Migration applied (N/A)
- [x] Tests written (N/A)
- [x] Implementation complete
- [ ] Quality gate passed

> **Deviation:** Also fixed pre-existing Config struct missing fields (`pipeline_timeout_secs`, `max_lfs_object_bytes`) in helpers/mod.rs, e2e_helpers/mod.rs, setup_integration.rs — these were preventing all test binaries from compiling.

### test-in-cluster.sh Changes

The script generates `RUN_ID` (line 64) but only uses it for namespace prefix. It should also use it for output files.

**Change output file naming:**

```bash
# Current (line 241):
REPORT_FILE="${PROJECT_DIR}/test-report.txt"

# Fix:
REPORT_FILE="${PROJECT_DIR}/test-report-${RUN_ID}.txt"
OUTPUT_FILE="${PROJECT_DIR}/test-output-${RUN_ID}.txt"
LOG_FILE="${PROJECT_DIR}/test-logs-${RUN_ID}.jsonl"
```

**Set TEST_LOG_FILE env var** so the tracing layer in `tests/helpers/mod.rs` writes structured JSON:

```bash
export TEST_LOG_FILE="${LOG_FILE}"
export RUST_LOG="${RUST_LOG:-platform=debug}"
```

**Capture nextest output** to `OUTPUT_FILE` (tee to both stdout and file):

```bash
# For single-tier runs, tee output:
cargo nextest run "${NEXTEST_ARGS[@]}" 2>&1 | tee "${OUTPUT_FILE}" || TEST_EXIT=$?

# For coverage runs:
cargo llvm-cov nextest "${COV_ARGS[@]}" "${NEXTEST_ARGS[@]}" 2>&1 | tee "${OUTPUT_FILE}" || TEST_EXIT=$?
```

**Print output file paths** at the end:

```bash
echo ""
echo "==> Test outputs:"
echo "    Report: ${REPORT_FILE}"
echo "    Output: ${OUTPUT_FILE}"
echo "    Logs:   ${LOG_FILE}"
```

### Coverage Defaults — Unit + Integration Only

**Justfile changes:**

| Recipe | Current | Change |
|---|---|---|
| `cov-total` | Runs `--type total` (unit + int + E2E) | Change to run unit + integration only (no E2E) |
| `cov-total` description | "unit + integration + E2E" | "unit + integration (needs cluster + DB)" |
| `cov-all` | keep as-is | This remains the "everything" coverage target |

In CLAUDE.md, update the command reference:
```
just cov-total      # ★ combined report: unit + integration (needs cluster + DB)
just cov-all        # all tiers combined (unit + int + E2E) → coverage-all.lcov
```

**Implementation in test-in-cluster.sh:**

The `--type total` handler currently runs 3 tiers. Change to run only unit + integration:

```bash
if [[ "$TEST_TYPE" == "total" ]]; then
  # Combined coverage: unit + integration
  TIER_FAILURES=0

  echo "==> Running unit tests (coverage, no report)"
  cargo llvm-cov nextest --no-report --lib ...

  echo "==> Running integration tests (coverage, no report)"
  cargo llvm-cov nextest --no-report --test '*_integration' ...

  # REMOVED: E2E tier (use cov-all for that)
```

### helpers/mod.rs — Ensure init_test_tracing is called

The `TestNameJsonFormat` and `init_test_tracing()` already exist in `tests/helpers/mod.rs` (survived the revert). Verify that `test_state()` calls `init_test_tracing()`. If not, add:

```rust
pub async fn test_state(pool: PgPool) -> (AppState, String) {
    init_test_tracing();  // ensure JSON logs with test.name
    // ... rest of setup
}
```

Same for `tests/e2e_helpers/mod.rs:e2e_state()`.

### CLAUDE.md Test Report Section Update

Update the "MANDATORY: Read test report" section to match the new file naming:

```markdown
| File | Content |
|---|---|
| **`test-report-{RUN_ID}.txt`** | Pass/fail summary |
| `test-output-{RUN_ID}.txt` | Full nextest output |
| `test-logs-{RUN_ID}.jsonl` | Structured JSON logs (filter by `test.name`) |
```

### Verification
- Run `just test-integration` → verify `test-report-{RUN_ID}.txt`, `test-output-{RUN_ID}.txt`, `test-logs-{RUN_ID}.jsonl` are created
- Verify `test-logs-*.jsonl` entries have `"test.name"` field
- Run `just cov-total` → verify only unit + integration run (no E2E)
- `grep '"test.name"' test-logs-*.jsonl | head -3` shows test names

---

## Plans Archival

11 completed plans should be archived per project policy. This is a separate low-priority task:

```bash
git rm plans/unified-platform.md plans/rust-dev-process.md plans/cicd-process-spec-v2.md \
  plans/codebase-audit-2026-03-24.md plans/ecosystem-audit-2026-03-24.md \
  plans/security-audit-2026-03-24.md plans/ecosystem-audit-fixes.md \
  plans/ecosystem-audit-fixes-e16-e30.md plans/ecosystem-audit-fixes-e31-e38.md \
  plans/ecosystem-audit-fixes-e39-e62.md plans/security-hardening-s1-s20.md
```

These remain in git history. Defer to after all fixes are applied and verified.

---

## Cascading Impact Summary

| System | Impact | PR |
|---|---|---|
| **CLAUDE.md** | Major content update (counts, AppState, commands, env vars) | PR 1 |
| **docs/ (13 files)** | Number updates across all arc42 + architecture + testing + feature-inventory | PR 1 |
| **MCP servers (2 files)** | Field name fixes, schema corrections | PR 2 |
| **Templates (3 files)** | Clarify template vs stub status | PR 2 |
| **Rust source (8 files)** | Remove dead_code allows, add module docs | PR 3 |
| **test-in-cluster.sh** | RUN_ID output files, TEST_LOG_FILE, coverage scope | PR 4 |
| **Justfile** | cov-total description update | PR 4 |
