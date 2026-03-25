# Skill: Documentation Audit — Accuracy & Freshness Review

**Description:** Orchestrates 7 parallel AI agents that verify every piece of documentation against the actual code. Covers ALL doc surfaces: root `CLAUDE.md`, `docs/` folder, template CLAUDE.md files (agent-facing), `plans/` directory, inline Rust doc comments, MCP/UI/Helm docs, AND infrastructure file comments (`Justfile`, `Cargo.toml`, `Chart.yaml`, `hack/*.sh`, `deploy/`, `docker/`, `.github/`). The core question: *"If someone follows these docs, will they succeed or hit a wall?"*

**When to use:** After significant refactoring, before a release, when docs haven't been updated in a while, or when onboarding new contributors. This is the audit that catches the gap between what the docs *say* and what the code *does*.

### Source of Truth Principle

**Code is always the source of truth.** Documentation is a *claim* about code. Every agent must treat the following as authoritative reality:

| Source of Truth | What it proves |
|---|---|
| `src/**/*.rs` | Actual types, functions, behavior, env var reads, API routes |
| `Cargo.toml` / `Cargo.lock` | Actual dependencies and versions |
| `Justfile` | Actual available commands and what they do |
| `helm/platform/templates/` + `values.yaml` | Actual Helm deployment config |
| `deploy/base/` | Actual Kustomize deployment config |
| `docker/Dockerfile*` | Actual build steps, base images, installed tools |
| `hack/*.sh` | Actual test infrastructure setup |
| `mcp/servers/*.js` + `mcp/lib/*.js` | Actual MCP tool definitions and client behavior |
| `ui/src/**/*.ts{x}` | Actual UI API calls, types, components |
| `migrations/*.sql` | Actual database schema |
| `.github/workflows/*.yaml` | Actual CI/CD pipeline |

When a doc says X and the code says Y — the doc is wrong, not the code.

---

## Orchestrator Instructions

You are the **Documentation Auditor**. Your job is to:

1. Inventory all documentation files
2. Launch 7 parallel agents that cross-reference docs against code
3. Collect and synthesize findings into a prioritized report
4. Produce a persistent `plans/docs-audit-<date>.md` report

### Severity Levels

| Severity | Meaning | Action |
|---|---|---|
| **CRITICAL** | Doc instructs something that will break/crash/lose data | Fix immediately |
| **HIGH** | Doc references non-existent API, env var, file, or pattern | Fix before anyone follows it |
| **MEDIUM** | Doc is misleading, incomplete, or uses outdated terminology | Fix when touching the area |
| **LOW** | Minor wording, formatting, or style issue | Fix only if trivial |

---

## Phase 0: Inventory

```bash
# All documentation files
echo "=== Markdown docs ==="
find . -name '*.md' -not -path '*/node_modules/*' -not -path '*/target/*' -not -path './.git/*' | sort

echo "=== Doc comments (top 20 files by count) ==="
grep -rl '///' src/ --include='*.rs' | head -20

echo "=== Plans directory ==="
ls -la plans/

echo "=== Template docs ==="
find src -name 'CLAUDE.md' -o -name '*.md' | sort
```

---

## Phase 1: Parallel Documentation Audits

Launch **all 7 agents concurrently**. Each agent reads documentation AND the code it references, then flags mismatches.

**Critical instructions for EVERY agent prompt:**
- READ the documentation file(s) completely
- For every claim (env var, function name, API endpoint, pattern), VERIFY it against the actual code
- Output format: `[SEVERITY] doc-file:line — claim vs reality\n  Doc says: ...\n  Code says: ...\n  Fix: ...`
- Agent is performing an AUDIT (read-only) — it must NOT edit any files

---

### Agent 1: Root CLAUDE.md — Guidelines vs Reality

**Scope:** `/CLAUDE.md` cross-referenced against actual code

**Read the entire CLAUDE.md, then verify EVERY claim:**

_Architecture rules:_
- [ ] Module list matches actual `src/` directory structure
- [ ] `AppState` struct definition matches actual `src/main.rs` or wherever it's defined
- [ ] Module boundary rules match actual imports (`grep -r "use crate::" src/`)

_Command reference:_
- [ ] Every `just` command listed actually exists in `Justfile`
- [ ] Command descriptions match what they actually do
- [ ] Any `just` commands in Justfile NOT documented in CLAUDE.md?

_Auth & RBAC patterns:_
- [ ] `AuthUser` extractor signature matches actual code
- [ ] `require_admin()`, `require_project_read()`, `require_project_write()` exist and have documented signatures
- [ ] Permission enum values listed match actual `Permission` enum
- [ ] Audit logging pattern (`AuditEntry` struct) matches actual struct fields

_Security patterns:_
- [ ] `validation::check_*` functions listed all exist with documented signatures
- [ ] Field limits table matches actual validation constants
- [ ] Rate limiting pattern matches actual implementation
- [ ] SSRF protection description matches actual `validate_webhook_url()`
- [ ] Webhook security description matches actual implementation

_Env vars table:_
- [ ] Every env var listed exists in `src/config.rs`
- [ ] Default values match
- [ ] Any env vars in `src/config.rs` NOT listed in CLAUDE.md?

_API module files:_
- [ ] Every file listed exists at the documented path
- [ ] Module descriptions match actual content

_Testing standards:_
- [ ] Test commands match Justfile recipes
- [ ] Test helper functions (`test_state`, `test_router`, etc.) have documented signatures
- [ ] Test patterns match actual test files

_Crate API gotchas:_
- [ ] Each gotcha is still relevant (check the actual crate versions in Cargo.toml)
- [ ] Any NEW gotchas discovered in code that should be documented?

_Error handling, observability, type system patterns:_
- [ ] Patterns described match actual usage in code
- [ ] Example code compiles (types, imports, function signatures correct)

**Output:** Numbered findings with doc line number, what it says, what code says, and fix.

---

### Agent 2: Architecture & Design Docs

**Scope:** `docs/architecture.md`, `docs/testing.md`, `docs/fe-be-testing.md`, `docs/design-decisions.md`, and any other files under `docs/`

**Read ALL docs files. For each document, verify against code:**

_Architecture doc:_
- [ ] Module diagram/description matches actual `src/` structure
- [ ] Data flow descriptions match actual implementations
- [ ] Component relationships accurate
- [ ] Technology choices listed match actual dependencies (Cargo.toml, package.json)
- [ ] Port numbers, protocols, storage paths accurate

_Testing doc:_
- [ ] Test tier boundaries match actual test file locations
- [ ] Test helper descriptions match actual helpers in `tests/helpers/` and `tests/e2e_helpers/`
- [ ] Coverage commands match Justfile recipes
- [ ] Test infrastructure description matches `hack/` scripts

_Frontend-backend testing doc:_
- [ ] API endpoints referenced exist
- [ ] Test patterns described match actual UI test setup
- [ ] Mock patterns match actual usage

_Design decisions:_
- [ ] Each decision's context is still accurate
- [ ] Referenced code/config still exists
- [ ] Decisions haven't been superseded by later changes

**Output:** Numbered findings.

---

### Agent 3: Template CLAUDE.md Files (Agent-Facing Docs)

**Scope:** `src/git/templates/CLAUDE.md`, `src/onboarding/templates/CLAUDE.md`

These are the docs that AI agents see when working on user projects. Incorrect docs here cause agent failures.

**Read both template files AND the platform API they reference:**

_API endpoint references:_
- [ ] Every API endpoint URL mentioned exists in `src/api/mod.rs` router
- [ ] Request/response shapes described match actual handler types
- [ ] HTTP methods correct (GET/POST/PATCH/PUT/DELETE)

_Configuration references:_
- [ ] `.platform.yaml` schema described matches `src/pipeline/definition.rs` parser
- [ ] Env var references match what agent pods actually receive
- [ ] File paths referenced exist in the template structure

_Workflow instructions:_
- [ ] Build/test/deploy instructions actually work with current tooling
- [ ] Pipeline step types listed match `StepType` enum
- [ ] Deployment model description matches current deployer behavior

_Feature descriptions:_
- [ ] Features described as available actually work
- [ ] Features removed/changed since doc was written
- [ ] Progressive delivery instructions match current implementation

**Output:** Numbered findings — these are high-priority since agents follow them blindly.

---

### Agent 4: Plans Directory — Staleness & Accuracy

**Scope:** All files under `plans/`

**Read EVERY plan file and classify:**

_Classification:_
- [ ] Which plans are completed and implemented? (Verify by checking if described changes exist in code)
- [ ] Which plans are in-progress? (Partially implemented)
- [ ] Which plans are stale/abandoned? (Described changes not in code, no recent git activity)
- [ ] Which plans reference files/functions that no longer exist?

_Accuracy of active plans:_
- [ ] Do in-progress plans reference correct file paths?
- [ ] Are code snippets in plans still valid?
- [ ] Do plans reference the right env vars, types, and patterns?

_Recommendations:_
- [ ] Which plans should be archived (moved out or deleted)?
- [ ] Which plans need updating to reflect current code?

**Output:** Table of all plans with status and recommended action.

---

### Agent 5: Inline Documentation (Rust doc comments)

**Scope:** All `src/**/*.rs` files — focus on `///` doc comments and `//!` module docs

**Scan for doc comment accuracy:**

_Module-level docs (`//!`):_
- [ ] Each `src/<module>/mod.rs` has a module doc comment
- [ ] Module doc accurately describes what the module does
- [ ] Module doc lists key types/functions that are still current

_Function/struct docs (`///`):_
- [ ] Do documented parameters match actual parameters?
- [ ] Do documented return types match actual return types?
- [ ] Do documented panics/errors match actual error paths?
- [ ] Are there `# Examples` that would compile?
- [ ] Are there doc comments on functions that have completely changed behavior?

_Missing docs:_
- [ ] Public functions/types without any doc comment (focus on `src/api/` and `src/auth/` — the surfaces others interact with)

_Stale `#[allow(...)]` annotations:_
- [ ] `#[allow(dead_code)]` on items that are actually used now
- [ ] `#[allow(unused_*)]` that are no longer needed

**Output:** Numbered findings. Focus on high-traffic modules (api, auth, rbac, agent, pipeline).

---

### Agent 6: MCP & UI Documentation

**Scope:** `mcp/` (any README or inline docs), `ui/` (any README or inline docs), `install.sh` inline comments, `helm/platform/NOTES.txt`

_MCP server docs:_
- [ ] Tool descriptions in each MCP server match actual behavior
- [ ] Input schema descriptions accurate
- [ ] Required vs optional fields correct
- [ ] Any tools that exist but aren't documented?

_UI component docs:_
- [ ] Any component-level comments that are stale?
- [ ] Type definitions in `ui/src/lib/types.ts` — do comments match actual API?

_install.sh:_
- [ ] Inline comments describe actual behavior
- [ ] Version numbers in comments match pinned versions
- [ ] Prerequisites listed are accurate

_Helm NOTES.txt:_
- [ ] Post-install instructions are accurate
- [ ] URLs and commands work with current chart structure

**Output:** Numbered findings.

---

### Agent 7: Infrastructure File Comments & Metadata

**Scope:** `Justfile`, `Cargo.toml`, `helm/platform/Chart.yaml`, `docker/Dockerfile*`, `hack/*.sh`, `deploy/base/*.yaml`, `.github/workflows/*.yaml`, `.pre-commit-config.yaml`, `deny.toml`, `.gitleaks.toml`

These files contain inline comments, descriptions, and metadata that serve as documentation. Verify them against reality.

_Justfile:_
- [ ] Recipe comments (`# description above recipe`) accurately describe what the recipe does
- [ ] Any recipes with stale comments after behavior changed?
- [ ] Missing comments on complex recipes?
- [ ] Variable defaults in comments match actual defaults

_Cargo.toml:_
- [ ] `description`, `keywords`, `categories` still accurate
- [ ] Feature flags documented (comments next to `[features]`)
- [ ] Dependency comments (e.g., `# pinned because...`) still relevant
- [ ] Crate version in comments matches actual version

_Helm Chart.yaml:_
- [ ] `appVersion` matches actual platform version
- [ ] `description` accurate
- [ ] Dependency versions in `Chart.yaml` match what's deployed

_Dockerfiles:_
- [ ] Stage comments (`# Stage 1: planner`, etc.) match actual stage purpose
- [ ] Version comments match pinned versions (e.g., `# kubectl v1.32.3`)
- [ ] `EXPOSE` ports match actual listened ports
- [ ] `ENV` comments describe actual behavior

_hack/ scripts:_
- [ ] Script header comments (usage, purpose) match actual behavior
- [ ] Inline comments in `cluster-up.sh`, `test-in-cluster.sh`, `deploy-services.sh` accurate
- [ ] Port numbers in comments match actual port mappings
- [ ] Referenced file paths in comments still exist

_deploy/ manifests:_
- [ ] Comments in `configmap.yaml`, `deployment.yaml`, `secret.yaml` accurate
- [ ] Resource names in comments match actual resource names
- [ ] Any `TODO` or `FIXME` comments that should have been resolved?

_CI/CD workflows:_
- [ ] Job descriptions (`name:` fields) match actual behavior
- [ ] Step comments accurate
- [ ] Trigger descriptions match actual trigger config

_Security tooling:_
- [ ] `deny.toml` comments describe actual ban reasons
- [ ] `.gitleaks.toml` allowlist descriptions match actual patterns
- [ ] `.pre-commit-config.yaml` hook descriptions accurate

**Output:** Numbered findings with file:line, stale comment, actual behavior, and fix.

---

## Phase 2: Synthesis

Once all 7 agents return, synthesize into a single report.

### Synthesis rules

1. **Deduplicate** — merge findings about the same stale reference
2. **Prioritize** — docs that cause failures (wrong API, wrong env var) over style issues
3. **Categorize** — group by:
   - **Dangerous misinformation** — following the doc causes errors/failures
   - **Stale references** — doc references things that no longer exist
   - **Missing documentation** — important features/patterns undocumented
   - **Accuracy drift** — doc is approximately right but details are wrong
   - **Cosmetic** — formatting, wording, style
4. **Number every finding** — D1, D2, D3... (D for Documentation)

---

## Phase 3: Write Audit Report

Persist as `plans/docs-audit-<YYYY-MM-DD>.md`.

### Report structure

```markdown
# Documentation Audit Report

**Date:** <today>
**Scope:** All documentation — CLAUDE.md, docs/, templates, plans/, inline docs, MCP/UI docs
**Auditor:** Claude Code (automated)
**Doc file count:** N files, ~N lines

## Executive Summary
- Documentation health: GOOD / NEEDS ATTENTION / SIGNIFICANTLY STALE
- Findings: X critical, Y high, Z medium, W low
- Stalest area: {which docs are most out of date}
- Best maintained: {which docs are most accurate}

## Documentation Inventory

| Doc File | Lines | Last Relevant | Status | Findings |
|---|---|---|---|---|
| CLAUDE.md | N | recent/stale | ✓/⚠/✗ | D1, D5 |
| docs/architecture.md | N | ... | ... | ... |
| ... | ... | ... | ... | ... |

## Plans Directory Status

| Plan | Status | Action |
|---|---|---|
| plans/foo.md | Completed | Archive |
| plans/bar.md | In-progress | Update file refs |
| plans/baz.md | Stale | Delete |

## Critical & High Findings

### D1: [CRITICAL/HIGH] {title}
- **Doc:** `path/doc.md:42`
- **Doc says:** {quoted claim}
- **Code says:** {actual state in code, with file:line}
- **Impact:** {what goes wrong if someone follows the doc}
- **Fix:** {specific edit}

## Medium & Low Findings
...

## Recommended Action Plan
### Immediate
1. Fix dangerous misinformation (D1-DN)
### Short-term
1. Update stale references
### Ongoing
1. Archive completed plans
2. Add doc validation to CI (optional)
```

---

## Phase 4: Summary to User

1. Documentation health (one sentence)
2. Finding counts by severity
3. Top 3 most dangerous doc errors
4. Which docs are most stale
5. Plans directory cleanup recommendations
6. Path to the full report file
