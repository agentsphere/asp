# Skill: API Design Audit — Consistency, Conventions & Contract Quality

**Description:** Orchestrates 4 parallel AI agents that audit the HTTP API for design consistency: naming conventions, HTTP method usage, status codes, error format, pagination, response envelopes, and REST contract quality. Unlike `/audit` (code quality) or `/audit-ecosystem` (integration contracts), this focuses on *"Is the API well-designed and internally consistent?"*

**When to use:** Before publishing API docs, before opening the API to third-party consumers, after adding many endpoints, or when API consumers report confusing behavior.

---

## Orchestrator Instructions

You are the **API Design Auditor**. Your job is to:

1. Inventory all API routes
2. Launch 4 parallel agents analyzing different API design dimensions
3. Synthesize findings into a prioritized report
4. Produce a persistent `plans/api-audit-<date>.md` report

### Severity Levels

| Severity | Meaning | Action |
|---|---|---|
| **CRITICAL** | API returns wrong data or wrong status code causing client failures | Fix immediately |
| **HIGH** | Inconsistent contract (same pattern handled differently across endpoints) | Fix before exposing to third parties |
| **MEDIUM** | Convention violation, missing error detail, suboptimal design | Fix when touching the endpoint |
| **LOW** | Minor naming nit, optional improvement | Fix only if trivial |

---

## Phase 0: Route Inventory

```bash
# All API routes
echo "=== API Routes ==="
grep -rn '\.route\|\.nest\b' src/api/mod.rs src/api/*.rs --include='*.rs' | head -80

# Handler function signatures
echo "=== Handler signatures ==="
grep -rn 'pub async fn\|async fn' src/api/*.rs --include='*.rs' | head -80

# Response types
echo "=== Response types ==="
grep -rn 'Json<\|StatusCode\|Response' src/api/*.rs --include='*.rs' | grep 'Result<' | head -40

# Error types
echo "=== ApiError variants ==="
grep -rn 'ApiError::' src/api/*.rs --include='*.rs' | sed 's/.*ApiError::/ApiError::/' | sort | uniq -c | sort -rn | head -20
```

---

## Phase 1: Parallel API Design Audits

Launch **all 4 agents concurrently**.

---

### Agent 1: URL Design & HTTP Method Conventions

**Scope:** `src/api/mod.rs` (router), all `src/api/*.rs` handler files

**Read the router and ALL handler files, then check:**

_URL structure:_
- [ ] Consistent use of plural nouns for collections (`/projects`, `/users`, not `/project`, `/user`)
- [ ] Consistent nesting depth (prefer `/projects/{id}/issues` over `/projects/{id}/issues/{issue_id}/comments/{comment_id}`)
- [ ] No verbs in URLs (prefer `POST /projects/{id}/deploy-releases/{id}/promote` over `POST /promote-release`)
- [ ] Consistent use of kebab-case or snake_case (don't mix)
- [ ] Resource IDs consistently named (`{id}` vs `{project_id}` — prefer contextual names)
- [ ] No trailing slashes in route definitions

_HTTP methods:_
- [ ] GET for reads (no side effects)
- [ ] POST for creation (returns 201 + created resource)
- [ ] PATCH for partial updates (only send changed fields)
- [ ] PUT for full replacements or idempotent operations
- [ ] DELETE for deletion (returns 204 or 200)
- [ ] No GET endpoints that mutate state
- [ ] No POST endpoints that could be GET (read-only operations)
- [ ] Consistent method choice for similar operations across different resources

_Status codes:_
- [ ] 200 for successful GET/PATCH/PUT
- [ ] 201 for successful POST (creation)
- [ ] 204 for successful DELETE (or 200 with body)
- [ ] 400 for validation errors
- [ ] 401 for missing/invalid auth
- [ ] 403 for insufficient permissions (or 404 to hide existence)
- [ ] 404 for not found
- [ ] 409 for conflicts (duplicate name, invalid state transition)
- [ ] 422 for semantically invalid input
- [ ] 429 for rate limiting
- [ ] Consistent status codes for the same situation across endpoints

_Route organization:_
- [ ] Logical grouping under prefixes (`/api/admin/*`, `/api/observe/*`)
- [ ] Consistent nesting for sub-resources
- [ ] No orphan routes (routes without a clear resource owner)

**Output:** Numbered findings with route, method, expected vs actual, and fix.

---

### Agent 2: Request/Response Format Consistency

**Scope:** All `src/api/*.rs` — request and response struct definitions

**Read ALL handler files, identify all request/response types, then check:**

_Response envelope consistency:_
- [ ] List endpoints: ALL use `ListResponse<T> { items: Vec<T>, total: i64 }` (or document exceptions)
- [ ] Single-item endpoints: ALL return the resource directly (not wrapped)
- [ ] Creation endpoints: return the created resource (not just an ID)
- [ ] Deletion endpoints: consistent return (204 no body, or 200 with deleted resource)
- [ ] Any endpoints returning bare `Vec<T>` that should use `ListResponse<T>`?
- [ ] Any endpoints returning bare `String` or `()` that should return structured JSON?

_Pagination consistency:_
- [ ] ALL list endpoints accept `limit` and `offset` query params
- [ ] Default limit is 50 everywhere (or document exceptions)
- [ ] Max limit is 100 everywhere (or document exceptions)
- [ ] Default offset is 0 everywhere
- [ ] Pagination enforced even without explicit params (no unbounded lists)

_Field naming:_
- [ ] Consistent snake_case for all JSON fields
- [ ] Consistent naming across resources:
  - `id` for primary key (not `user_id` in user response)
  - `created_at`, `updated_at` for timestamps
  - `name`, `display_name`, `description` for common fields
- [ ] Date/time format: ISO 8601 everywhere
- [ ] UUID format: lowercase with hyphens everywhere

_Request validation:_
- [ ] Required vs optional fields consistent with business logic
- [ ] Consistent error message format for validation failures
- [ ] Same field validated the same way across endpoints (name length, email format)
- [ ] No silent field ignoring (if a field is unknown, reject or document)

_Null/empty handling:_
- [ ] Consistent treatment of `null` vs missing field vs empty string
- [ ] Optional fields: `Option<T>` in request vs `#[serde(skip_serializing_if = "Option::is_none")]` in response
- [ ] Empty lists: return `[]` not `null`

**Output:** Numbered findings with endpoint, inconsistency, and fix.

---

### Agent 3: Error Response Quality

**Scope:** `src/error.rs`, all `src/api/*.rs` error handling

**Read error types and ALL error paths in handlers:**

_Error format consistency:_
- [ ] All errors return JSON (not plain text, not HTML)
- [ ] Consistent error shape: `{ "error": "message" }` or `{ "error": "type", "message": "detail" }`
- [ ] Error messages are user-facing quality (not internal debug messages)
- [ ] No stack traces or internal paths in error responses
- [ ] No SQL error details in error responses

_Error type coverage:_
- [ ] Every handler has explicit error mapping (not just `?` propagation to a generic 500)
- [ ] Validation errors include which field failed and why
- [ ] Auth errors distinguish between "not authenticated" (401) and "not authorized" (403/404)
- [ ] Resource not found errors include the resource type
- [ ] Conflict errors explain what conflicted

_Error consistency across endpoints:_
- [ ] Same type of error returns the same status code everywhere
- [ ] Same validation failure returns the same error message everywhere
- [ ] Auth failures handled the same way across all endpoints

_ApiError enum:_
- [ ] Every variant maps to a clear HTTP status code
- [ ] No `Internal(String)` used for known error conditions (should have specific variants)
- [ ] No `anyhow::Error` exposed directly to clients
- [ ] Error chain preserved for logging but not for client response

_Error documentation:_
- [ ] Are possible error responses documented for each endpoint? (Even if just in code comments)
- [ ] Can API consumers predict what errors they'll get?

**Output:** Numbered findings with error path, current behavior, expected behavior, and fix.

---

### Agent 4: API Completeness & CRUD Symmetry

**Scope:** All `src/api/*.rs`, `src/api/mod.rs`

**Check API completeness for each resource:**

_CRUD symmetry:_
For each resource (projects, issues, MRs, webhooks, secrets, users, roles, sessions, pipelines, deployments, workspaces, etc.):
- [ ] CREATE exists → does GET (single) exist?
- [ ] CREATE exists → does LIST exist?
- [ ] CREATE exists → does UPDATE exist?
- [ ] CREATE exists → does DELETE exist?
- [ ] If any CRUD op is missing — is it intentional? (Document why)

_Sub-resource completeness:_
- [ ] Comments: create, list, update, delete for issues AND MRs?
- [ ] Reviews: create, list for MRs?
- [ ] Webhooks: create, list, update, delete for projects?
- [ ] Targets/releases: full CRUD?

_Filter & search:_
- [ ] List endpoints: what filters are available? Are obvious filters missing?
- [ ] Consistency: if issues can be filtered by `status`, can MRs also be filtered by `status`?
- [ ] Sort: is sorting available on list endpoints? Consistent sort options?

_Bulk operations:_
- [ ] Are there places where bulk operations would be useful but only single-item operations exist?
- [ ] Example: bulk label update, bulk status change, bulk delete

_Idempotency:_
- [ ] POST/creation: is the response the same if called twice with same data?
- [ ] PUT: is it truly idempotent (same result on retry)?
- [ ] DELETE: is it idempotent (204 on already-deleted resource, or 404)?

_Content negotiation:_
- [ ] All endpoints accept `application/json` for POST/PATCH/PUT
- [ ] All endpoints return `application/json` (with `Content-Type` header)
- [ ] Binary endpoints (git, registry, LFS) properly set content types

**Output:** Numbered findings with resource, missing operation, and recommendation.

---

## Phase 2: Synthesis

Deduplicate, prioritize, categorize:
- **Contract violations** — endpoints that don't follow their own patterns
- **Inconsistencies** — same thing done differently across endpoints
- **Missing features** — CRUD gaps, missing filters
- **Error quality** — unclear or inconsistent error responses
- **Design debt** — patterns that will confuse third-party consumers

Number findings API1, API2, API3...

---

## Phase 3: Write Report

Persist as `plans/api-audit-<YYYY-MM-DD>.md`.

### Report structure

```markdown
# API Design Audit Report

**Date:** <today>
**Scope:** N API endpoints across N handler files
**Auditor:** Claude Code (automated)

## Executive Summary
- API design health: GOOD / NEEDS ATTENTION / INCONSISTENT
- Findings: X critical, Y high, Z medium, W low
- Consistency score: N/10

## Route Inventory

| Method | Path | Handler | Auth | Pagination | Status |
|---|---|---|---|---|---|
| GET | /api/projects | list_projects | AuthUser | ✓ | ✓ |
| POST | /api/projects | create_project | AuthUser | — | ✓ |
| ... | ... | ... | ... | ... | ... |

## CRUD Completeness Matrix

| Resource | Create | Read | List | Update | Delete | Notes |
|---|---|---|---|---|---|---|
| projects | ✓ | ✓ | ✓ | ✓ | ✓ (soft) | — |
| issues | ✓ | ✓ | ✓ | ✓ | ✓ | — |
| ... | ... | ... | ... | ... | ... | ... |

## Response Format Consistency

| Pattern | Consistent? | Violations |
|---|---|---|
| List → ListResponse<T> | ⚠ | API3 (tokens returns bare array) |
| Create → 201 + resource | ✓ | — |
| Delete → 204 | ⚠ | API7 |
| ... | ... | ... |

## Critical & High Findings
...

## Recommended Action Plan
...
```

---

## Phase 4: Summary to User

1. API design health (one sentence)
2. Finding counts
3. Top 3 inconsistencies
4. CRUD completeness summary
5. Error format consistency summary
6. Path to report
