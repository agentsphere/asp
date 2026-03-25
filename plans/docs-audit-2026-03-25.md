# Documentation Audit Report

**Date:** 2026-03-25
**Scope:** All documentation — CLAUDE.md, docs/, templates, plans/, inline docs, MCP/UI/Helm docs, infrastructure comments
**Auditor:** Claude Code (automated, 7 parallel agents)
**Doc file count:** 35+ files across 7 doc surfaces

## Executive Summary

- **Documentation health: NEEDS ATTENTION**
- The root CLAUDE.md is the most maintained doc but has 6 errors (wrong module count, incomplete AppState, stale commands). The `docs/` folder has pervasive stale counts (LOC, migrations, tests, modules). Template CLAUDE.md files have 3 HIGH issues that will cause agent build failures. MCP deploy server has 4 contract mismatches from the recent API rewrite. 11 of 16 plans are completed and should be archived per project policy.
- **Findings: 8 critical, 18 high, 22 medium, 19 low**
- **Stalest area:** `docs/` folder — LOC counts (23K vs 71K), migration counts (24 vs 64), module counts (11 vs 15), test file counts all wrong
- **Best maintained:** CLAUDE.md security patterns, auth/RBAC patterns, crate API gotchas — all verified accurate

## Documentation Inventory

| Doc File | Status | Findings |
|---|---|---|
| CLAUDE.md | ⚠ Needs update | D1-D10 |
| docs/architecture.md | ⚠ Stale counts | D11-D16 |
| docs/testing.md | ⚠ Stale counts | D17-D21 |
| docs/feature-inventory.md | ⚠ Stale counts | D22-D28 |
| docs/arc42/* (13 files) | ⚠ Stale counts | D29-D35 |
| docs/design-decisions.md | ✓ Accurate | — |
| docs/cicd-process-spec.md | ✓ Accurate (self-documenting) | — |
| src/git/templates/CLAUDE.md | ⚠ Build failures | D36-D40 |
| src/onboarding/templates/CLAUDE.md | ✓ Accurate | — |
| seed-commands/dev.md | ⚠ Stale refs | D41-D42 |
| MCP servers (7 files) | ⚠ Deploy contract drift | D43-D50 |
| README.md | ⚠ Stale refs/counts | D51-D52 |
| Inline Rust docs | ⚠ Stale #[allow] | D53-D59 |
| Infrastructure comments | ⚠ Minor issues | D60-D67 |

## Plans Directory Status

| Plan | Type | Status | Action |
|---|---|---|---|
| unified-platform.md | Architecture reference | Completed | **Archive** |
| rust-dev-process.md | Toolchain reference | Completed | **Archive** |
| cicd-process-spec-v2.md | CI/CD spec | Completed | **Archive** |
| codebase-audit-2026-03-24.md | Audit report | Completed | **Archive** |
| ecosystem-audit-2026-03-24.md | Audit report | Completed | **Archive** |
| security-audit-2026-03-24.md | Audit report | Completed | **Archive** |
| ecosystem-audit-fixes.md | Fix plan (E1-E15) | Completed | **Archive** |
| ecosystem-audit-fixes-e16-e30.md | Fix plan (E16-E30) | Completed | **Archive** |
| ecosystem-audit-fixes-e31-e38.md | Fix plan (E31-E38) | Completed | **Archive** |
| ecosystem-audit-fixes-e39-e62.md | Fix plan (E39-E62) | Completed | **Archive** |
| security-hardening-s1-s20.md | Fix plan (S1-S20) | Completed (11/12, 1 deferred) | **Archive** |
| audit-a1-a15-fixes.md | Fix plan | In-progress (PR 1-2 done, PR 3-4 pending) | **Keep** |
| audit-a16-a25-fixes.md | Fix plan | In-progress (not started) | **Keep** |
| audit-a3-ssh-branch-protection.md | Fix plan | In-progress (not started) | **Keep** |
| progressive-delivery-hardening.md | Fix plan | In-progress (PR 1-2 done, PR 3 pending) | **Keep** |
| road-to-v0.1.md | Roadmap tracker | Active | **Keep** |

**11 plans should be archived** (completed work cluttering active plans directory).

---

## Critical Findings (8)

### D1: [CRITICAL] CLAUDE.md — Module count "11 modules" is wrong
- **Doc:** `CLAUDE.md:48`
- **Doc says:** "11 modules under src/"
- **Code says:** 15 subdirectories with `mod.rs`: api, auth, rbac, store, git, pipeline, deployer, agent, observe, secrets, notify, registry, workspace, onboarding, health
- **Impact:** Contributors underestimate codebase scope
- **Fix:** Change "11 modules" to "15 modules"

### D2: [CRITICAL] CLAUDE.md — AppState struct missing 6 fields
- **Doc:** `CLAUDE.md:49-59`
- **Doc says:** 7 fields (pool, valkey, minio, kube, config, webauthn, pipeline_notify)
- **Code says:** `src/store/mod.rs:19-40` has 13 fields — missing: deploy_notify, secret_requests, cli_sessions, health, task_registry, cli_auth_manager
- **Impact:** New code may miss required AppState fields
- **Fix:** Add all 13 fields to the AppState example

### D3: [CRITICAL] docs/ — LOC count "~23K" is drastically wrong everywhere
- **Doc:** `docs/architecture.md:7`, `docs/arc42/02-constraints.md:9`, CLAUDE.md
- **Doc says:** "~23K LOC"
- **Code says:** `src/` is ~71K LOC
- **Impact:** Wildly misleads contributors about codebase size
- **Fix:** Update to "~71K LOC" in all docs

### D4: [CRITICAL] docs/ — Migration count "24 pairs" is wrong everywhere
- **Doc:** `docs/architecture.md:59` (24), `docs/arc42/02-constraints.md:12` (28), `docs/arc42/04-solution-strategy.md:8` (28)
- **Doc says:** 24 or 28 migration pairs
- **Code says:** 64 migration pairs in `migrations/`
- **Fix:** Update to "64 migration pairs" everywhere

### D5: [CRITICAL] git template CLAUDE.md — Default Project Structure lists files that don't exist
- **Doc:** `src/git/templates/CLAUDE.md:418-431`
- **Doc says:** `app/`, `app/main.py`, `app/db.py`, `app/models.py`, `app/routes.py`, `static/`, `requirements.txt` exist
- **Code says:** Template only ships 12 files (see `src/git/templates.rs:24-74`). None of these exist.
- **Impact:** Agents assume files exist and try to modify them, causing confusion
- **Fix:** Note these are "files to create" not "files that exist", or ship stubs

### D6: [CRITICAL] git template CLAUDE.md — Dockerfile references files not in template
- **Doc:** `src/git/templates/CLAUDE.md:10-11`
- **Doc says:** Dockerfile is the "Application container image"
- **Code says:** The Dockerfile does `COPY requirements.txt`, `COPY app/`, `COPY static/` — all absent from template. Docker build fails on fresh project.
- **Impact:** Agent's first `docker build` fails immediately
- **Fix:** Ship stub `requirements.txt`, `app/main.py`, `static/` or document they must be created first

### D7: [CRITICAL] git template CLAUDE.md — kubectl apply on minijinja template will fail
- **Doc:** `src/git/templates/CLAUDE.md:205,399`
- **Doc says:** `kubectl apply -f deploy/production.yaml`
- **Code says:** `deploy/production.yaml` contains `{{ project_name }}`, `{{ image_ref }}` — minijinja syntax, not valid K8s YAML
- **Impact:** Agent runs kubectl apply and gets parse errors
- **Fix:** Clarify agents should create their own manifests with literal values, or provide a non-template dev manifest

### D8: [CRITICAL] MCP deploy server — adjust_traffic sends wrong field name
- **Doc:** `mcp/servers/platform-deploy.js:126`
- **Doc says:** `body: { weight: args.weight }`
- **Code says:** `AdjustTrafficRequest { pub traffic_weight: i32 }` at `src/api/deployments.rs:103-105`
- **Impact:** Traffic adjustments silently do nothing (field ignored)
- **Fix:** Change to `{ traffic_weight: args.weight }`

---

## High Findings (18)

### D9: [HIGH] CLAUDE.md — `just test` recipe does not exist
- **Doc:** `CLAUDE.md:16`
- **Doc says:** `just test # cargo nextest run (all tests)`
- **Code says:** No `test` recipe in Justfile. Use `test-unit`, `test-integration`, `test-e2e`, or `test-all`.
- **Fix:** Remove or correct

### D10: [HIGH] CLAUDE.md — `cov-total` description is wrong
- **Doc:** `CLAUDE.md:35`
- **Doc says:** "combined report: unit + integration + E2E"
- **Code says:** Justfile `cov-total` passes `--type unit-int` — NO E2E. `cov-all` includes E2E.
- **Fix:** Change description to "unit + integration (no E2E)"

### D11: [HIGH] CLAUDE.md — check_container_image() location wrong
- **Doc:** `CLAUDE.md:235-237`
- **Doc says:** "in src/pipeline/definition.rs"
- **Code says:** Both `check_container_image()` and `check_setup_commands()` are in `src/validation.rs`
- **Fix:** Update location

### D12: [HIGH] CLAUDE.md — ExecutorGuard::spawn does not exist
- **Doc:** `CLAUDE.md:697`
- **Doc says:** `let _executor = ExecutorGuard::spawn(&state);`
- **Code says:** No `ExecutorGuard` type in `src/pipeline/`
- **Fix:** Update to actual executor spawn pattern

### D13: [HIGH] CLAUDE.md — MCP server count wrong
- **Doc:** `CLAUDE.md:334-343`
- **Doc says:** "6 MCP servers"
- **Code says:** 7 servers (missing `platform-browser.js`)
- **Fix:** Update count and add browser server to list

### D14: [HIGH] CLAUDE.md — Many Justfile recipes undocumented
- **Doc:** `CLAUDE.md:7-43`
- **Code says:** 15+ recipes missing: `dev-up`, `dev-down`, `dev`, `types`, `test-bin`, `test-llm`, `test-cleanup`, `build-agent-images`, `agent-image`, `agent-image-bare`, `agent-images`, `registry-login`, `docs-viewer`, `docs-serve`, etc.
- **Fix:** Add missing recipes to command reference

### D15: [HIGH] CLAUDE.md — Env vars table missing ~25 variables
- **Doc:** `CLAUDE.md:447-466`
- **Code says:** `src/config.rs` has ~50 env vars; table only lists ~25. Missing: `PLATFORM_LISTEN`, `DATABASE_URL`, `VALKEY_URL`, `MINIO_*`, `PLATFORM_SMTP_*`, `PLATFORM_SSH_*`, `PLATFORM_MAX_CLI_SUBPROCESSES`, `PLATFORM_VALKEY_AGENT_HOST`, `PLATFORM_AGENT_RUNNER_DIR`, `PLATFORM_MCP_SERVERS_TARBALL`, `PLATFORM_CLAUDE_CLI_VERSION`, `PLATFORM_NS_PREFIX`, many more.
- **Fix:** Add missing env vars

### D16: [HIGH] docs/testing.md — Test file counts stale
- **Doc:** `docs/testing.md:91,206`
- **Doc says:** 26 integration files, 5 E2E files
- **Code says:** 38 integration files, 9 E2E files
- **Fix:** Update all counts and file lists

### D17: [HIGH] docs/architecture.md — Module count and API file count wrong
- **Doc:** `docs/architecture.md:17,25`
- **Doc says:** 11 modules, 14 API handler modules
- **Code says:** 15 modules, 27+ API handler modules
- **Fix:** Update counts

### D18: [HIGH] docs/feature-inventory.md — Multiple stale file counts
- File counts wrong for: rbac (claims 5 files/has middleware.rs — actually 4, no middleware.rs), api (23→30), git (8→12), secrets (3→5), deployer (8→11), agent (21→23)
- **Fix:** Update all counts

### D19: [HIGH] docs/arc42/ — Stale counts across 6+ files
- Module count "11" appears in: arc42/02, arc42/04, arc42/09. Migration count "28" in arc42/02, arc42/03, arc42/04. Test count "1,339" in arc42/01, arc42/08, arc42/10.
- **Fix:** Global find-and-replace for stale numbers

### D20: [HIGH] Inline Rust — 5 stale #[allow(dead_code)] on widely-used items
- `src/config.rs:5` — Config struct: all fields consumed
- `src/store/mod.rs:18` — AppState struct: all fields consumed
- `src/error.rs:5` — ApiError enum: all variants consumed
- `src/secrets/mod.rs:2` — secrets::engine: used in 15+ locations
- `src/agent/pubsub_bridge.rs:14` — publish_event: called in 11+ locations
- **Fix:** Remove all 5 `#[allow(dead_code)]` annotations

### D21: [HIGH] Inline Rust — claude_cli stale TODO
- `src/agent/claude_cli/mod.rs:1-11` — "TODO(plan-37-pr5): Remove dead_code/unused_imports allowances when wiring is complete."
- All submodules are actively used. Wiring IS complete.
- **Fix:** Remove all `#[allow(dead_code)]` and the TODO comment

### D22: [HIGH] MCP deploy — create_target strategy uses "blue_green" but API expects "ab_test"
- `mcp/servers/platform-deploy.js:60`
- **Fix:** Change "blue_green" to "ab_test"

### D23: [HIGH] MCP deploy — create_release sends non-existent target_id field
- `mcp/servers/platform-deploy.js:104`
- **Code says:** `CreateReleaseRequest` has no target_id; handler auto-selects production target
- **Fix:** Remove target_id from schema

### D24: [HIGH] MCP deploy — create_target marks environment as required but API accepts Optional
- `mcp/servers/platform-deploy.js:66`
- **Fix:** Remove "environment" from required array

### D25: [HIGH] MCP admin — create_user user_type enum has "service" instead of "service_account"
- `mcp/servers/platform-admin.js:57`
- **Fix:** Change "service" to "service_account"

### D26: [HIGH] seed-commands/dev.md — References deploy/production.yaml which was deleted
- `seed-commands/dev.md:39,47`
- Templates now use versioned names: deployment-v0.1.yaml, deployment-v0.2.yaml
- **Fix:** Update references

---

## Medium Findings (22)

### D27-D35: docs/ stale details
- D27: architecture.md — preview cleanup interval says "15s" but runs within reconciler loop
- D28: architecture.md — "replacing 8+ services" but only 6 named
- D29: arc42/03 — "87 config fields" but actual count is ~52
- D30: arc42/05 — registry "proxy" sub-module listed but doesn't exist
- D31: arc42/06 — references nonexistent `plans/cicd-process-spec-v2.md` (actual: `docs/cicd-process-spec.md`) — Wait, plans/cicd-process-spec-v2.md DOES exist. But the referenced path from arc42/06 may differ.
- D32: arc42/06 — PipelineStatus state machine missing Pending→Cancelled and Pending→Failure transitions
- D33: feature-inventory.md — store says "5 files" but has 6
- D34: feature-inventory.md — notify says "3 files" but has 4
- D35: testing.md — CI recipe description incomplete (missing cli::lint, cli::test, mcp::test)

### D36-D40: Template docs
- D36: git template CLAUDE.md — canary traffic steps described as fixed (10→25→50→100) but are configurable via `.platform.yaml`
- D37: git template CLAUDE.md — requirements.txt listed as existing key file but not shipped in template
- D38: git template .claude/commands/dev.md — also tells agents to `kubectl apply` a template file
- D39: onboarding dev.md — missing push/MR/build verification steps (steps 7-8)
- D40: git template CLAUDE.md — COMMIT_SHA and REGISTRY env vars are conditional (may not be set)

### D41-D42: MCP
- D41: MCP deploy — list_releases sends target_id filter but API ignores it
- D42: MCP admin — create_user marks password as required but API accepts Optional (only required for human users)

### D43-D46: UI/README
- D43: UI types.ts — exports stale Deployment/DeploymentHistory types (should be DeployTarget/Release/ReleaseHistory)
- D44: README.md — references stale plan files (plans/01-foundation.md through plans/10-web-ui.md)
- D45: README.md — test counts (716, 574, 49) likely stale
- D46: CLAUDE.md — check_email comment says "1-254" but code enforces min=3

### D47-D48: Infrastructure
- D47: hack/test-in-cluster.sh:6 — references "OrbStack" but project uses Kind
- D48: docker/Dockerfile.platform-runner-bare:2-3 — claims "Contains only Node.js, git, curl, and sudo" but also includes Kaniko executor

---

## Low Findings (19)

- D49: CLAUDE.md — `just ci` description missing cli::lint, cli::test, mcp::test
- D50: CLAUDE.md — `just cov-summary` described as "unit + integration" but only runs unit
- D51: CLAUDE.md — `just run` description says "cargo run" but recipe takes env file arg
- D52: CLAUDE.md — UI lib file list: ws.ts doesn't exist (renamed to sse.ts), missing onboarding.tsx, webauthn.ts
- D53: CLAUDE.md — UI pages/components lists incomplete (many added since doc written)
- D54: CLAUDE.md — WebAuthn env var defaults shown as "—" but code defaults to localhost values
- D55: CLAUDE.md — inline permission example uses has_permission() but helpers use has_permission_scoped()
- D56: MCP admin — list_roles sends limit/offset but API ignores them
- D57: MCP admin — update_user description doesn't mention password change support
- D58: MCP deploy — create_target has "branch" field but API has no branch field
- D59: MCP observe — search_logs level enum includes "trace"/"fatal" which may not match stored data
- D60: docker/Dockerfile.dev-pod:6 — shows k3s import command but project uses Kind
- D61: docker/Dockerfile.dev-pod:65 — references `just test-ui-headless` which doesn't exist
- D62: hack/deploy-services.sh header omits preview-proxy from service list
- D63: install.sh:65 — kubectl `--short` flag deprecated
- D64: 14 module files lack `//!` module-level doc comments
- D65: src/auth/middleware.rs:12 — says AuthUser is "set as request extension" but it's a FromRequestParts extractor
- D66: 4 legacy dead functions in pipeline/executor.rs marked "kept temporarily" — should be cleaned up
- D67: .pre-commit-config.yaml check-yaml exclude pattern has no comment explaining why

---

## Recommended Action Plan

### Immediate (global number fixes)
1. **Find-and-replace stale counts** across all docs: module count 11→15, LOC 23K→71K, migrations 24/28→64
2. **Fix CLAUDE.md** AppState struct, command reference, env vars table, API module list
3. **Fix MCP deploy server** — 4 contract mismatches (D8, D22-D24)
4. **Fix git template CLAUDE.md** — project structure and kubectl instructions (D5-D7)

### Short-term
5. **Remove 5 stale `#[allow(dead_code)]`** on Config, AppState, ApiError, secrets::engine, publish_event
6. **Remove claude_cli TODO** and dead_code annotations
7. **Update docs/testing.md** file lists and counts
8. **Update docs/feature-inventory.md** file counts
9. **Fix seed-commands/dev.md** stale deploy/production.yaml reference
10. **Fix MCP admin** user_type enum and password requirement

### Ongoing
11. **Archive 11 completed plans** per project policy
12. **Re-generate UI types** (`just types`) to update stale Deployment types
13. **Add module-level `//!` doc comments** to all 14 modules missing them
