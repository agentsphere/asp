# Review: 44-iframe-preview-panels (PR 2)

**Date:** 2026-03-13
**Scope:** `src/agent/preview_watcher.rs` (new), `src/agent/provider.rs`, `src/agent/mod.rs`, `src/api/sessions.rs`, `src/main.rs`, `ui/src/lib/types.ts`, `ui/src/pages/SessionDetail.tsx`, `ui/src/components/AgentChatPanel.tsx`
**Overall:** PASS WITH FINDINGS

## Summary
- Clean K8s Service informer implementation with proper shutdown handling, error restart, and well-tested pure helper functions. ProgressKind enum extension is backward-compatible.
- 0 critical, 1 high, 4 medium, 4 low findings
- 11 unit tests added (8 preview_watcher + 3 provider); 0 integration tests for new handler (gap)
- Touched-line coverage: not measured; `list_iframes` handler entirely uncovered by tests

## Critical & High Findings (must fix)

### R1: [HIGH] `list_iframes` handler has zero integration tests
- **File:** `src/api/sessions.rs:903-955`
- **Domain:** Tests
- **Description:** The new `list_iframes` handler has 5 distinct code paths (happy path, auth failure, permission denial, session not found, project mismatch) with no integration tests. Every other handler in `sessions.rs` has integration tests in `tests/session_integration.rs`.
- **Risk:** Regressions in auth checks, permission enforcement, or error handling will not be caught.
- **Suggested fix:** Add 5 integration tests to `tests/session_integration.rs`:
  - `list_iframes_returns_empty_for_session` — happy path (empty result since no K8s Services exist in test)
  - `list_iframes_no_auth_returns_401` — GET without token
  - `list_iframes_no_permission_returns_404` — unprivileged user on private project
  - `list_iframes_nonexistent_session_returns_404` — random session UUID
  - `list_iframes_wrong_project_returns_404` — session belongs to different project

## Medium Findings (should fix)

### R2: [MEDIUM] `handle_service_applied` and `handle_service_deleted` are 90% duplicated
- **File:** `src/agent/preview_watcher.rs:59-108`
- **Domain:** Rust Quality
- **Description:** Both functions have identical logic: extract session_id, get service name, iterate iframe ports, build event, publish. Only the `ProgressKind` differs.
- **Suggested fix:** Extract a shared helper:
  ```rust
  async fn handle_service_event(state: &AppState, svc: &Service, kind: ProgressKind) { ... }
  ```

### R3: [MEDIUM] Missing `#[tracing::instrument]` on async functions with side effects
- **File:** `src/api/sessions.rs:903`, `src/agent/preview_watcher.rs:59,85`
- **Domain:** Observability
- **Description:** `list_iframes` performs K8s API calls and `handle_service_applied`/`handle_service_deleted` perform Valkey pub/sub, but none have tracing instrumentation. Other mutation handlers in sessions.rs have it.
- **Suggested fix:** Add `#[tracing::instrument(skip(state), fields(%id, %session_id), err)]` to `list_iframes` and `#[tracing::instrument(skip_all)]` to the watcher helpers.

### R4: [MEDIUM] `SessionDetail.tsx` normalizeKind missing `WaitingForInput` mapping
- **File:** `ui/src/pages/SessionDetail.tsx:18-28`
- **Domain:** UI
- **Description:** `SessionDetail.tsx`'s `normalizeKind` map does NOT include `waiting_for_input: 'WaitingForInput'` or `WaitingForInput: 'WaitingForInput'`. `AgentChatPanel.tsx` has them. This pre-dates PR 2 but was exposed when both files were updated for iframe events.
- **Suggested fix:** Add the missing entries to SessionDetail.tsx's normalizeKind map.

### R5: [MEDIUM] All-namespace watcher trusts K8s labels without namespace cross-check
- **File:** `src/agent/preview_watcher.rs:26-28`
- **Domain:** Security
- **Description:** The watcher uses `Api::all()` and trusts the `platform.io/session` label to route events. A Service in any namespace with a valid session UUID label could inject iframe events into that session's Valkey channel. Impact is limited to bogus notifications (no data exfiltration), and K8s RBAC restricts Service creation.
- **Suggested fix:** After extracting session_id, verify the Service's namespace matches the session's expected namespace. Alternatively, add `.page_size(100)` to the watcher config for defensive memory management. Given the controlled environment, this can be deferred.

## Low Findings (optional)

- [LOW] R6: `src/api/sessions.rs:924` — No namespace format validation before K8s API call. The `preview.rs` handler validates via `validate_namespace_format()` but `list_iframes` uses `session_namespace` directly. Defense-in-depth; namespace is constructed from validated slugs. Fix: add validation or extract shared helper.
- [LOW] R7: `src/agent/preview_watcher.rs:67-68` — Unnecessary `String` allocation: `.unwrap_or("unknown").to_owned()` when `build_iframe_event` takes `&str`. Fix: use `as_deref().unwrap_or("unknown")` without `.to_owned()`.
- [LOW] R8: `src/api/sessions.rs:934-952` — `filter_map().flatten()` allocates intermediate `Vec` per service. Fix: replace with `.flat_map()` returning an iterator directly.
- [LOW] R9: `src/agent/preview_watcher.rs` — Minor edge case tests missing: unnamed ports, multiple iframe ports on one Service, empty ports vec, non-iframe ProgressKind branch in `build_iframe_event`.

## Coverage — Touched Lines

| File | Lines changed | Lines covered | Coverage % | Uncovered lines |
|---|---|---|---|---|
| `src/agent/preview_watcher.rs` | 163 (new) | ~75 (unit-testable helpers) | ~46% | 22-56 (run loop), 59-108 (handlers) |
| `src/agent/provider.rs` | 36 | 36 | 100% | — |
| `src/agent/mod.rs` | 1 | — | N/A | Module declaration |
| `src/api/sessions.rs` | 72 | 0 | 0% | 886-955 (entire list_iframes) |
| `src/main.rs` | 4 | 0 | N/A | Background task spawn wiring |
| `ui/src/lib/types.ts` | 10 | N/A | N/A | TypeScript |
| `ui/src/pages/SessionDetail.tsx` | 4 | N/A | N/A | TypeScript |
| `ui/src/components/AgentChatPanel.tsx` | 2 | N/A | N/A | TypeScript |

### Uncovered Paths
- `src/api/sessions.rs:903-955` — Entire `list_iframes` handler (0% coverage); needs 5 integration tests (R1)
- `src/agent/preview_watcher.rs:22-56` — `run()` loop; requires live K8s watcher (covered by E2E only)
- `src/agent/preview_watcher.rs:59-108` — `handle_service_applied/deleted`; requires live Valkey pub/sub (covered by E2E only)

## Checklist Results

| Category | Status | Notes |
|---|---|---|
| Error handling | PASS | Proper use of `?`, `map_err`, no unwrap in production |
| Auth & permissions | PASS | `require_project_read` + session-project ownership check |
| Input validation | PASS | UUID path params type-safe; namespace from DB |
| Audit logging | N/A | Read-only endpoint — no mutations |
| Tracing instrumentation | FAIL | Missing `#[tracing::instrument]` on list_iframes and watcher helpers (R3) |
| Clippy compliance | PASS | Clean |
| Test patterns | FAIL | Zero integration tests for new handler (R1) |
| Migration safety | N/A | No migrations in PR 2 |
| Backward compatibility | PASS | `#[serde(other)] Unknown` remains last variant |
| UI consistency | FAIL | SessionDetail.tsx normalizeKind diverges from AgentChatPanel.tsx (R4) |
