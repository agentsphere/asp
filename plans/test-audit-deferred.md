# Plan: Deferred Test Audit Fixes — T2, T3, T9

**Source:** `plans/test-audit-fixes.md` deferred items
**Scope:** `enforce_merge_gates()` tests, auto-merge endpoints, rate limit boundaries
**Tier:** All integration (single endpoint + side effects, needs Kind cluster for Postgres/Valkey)

---

## Prerequisites: Shared Helpers

Before writing tests, extract two local helpers to `tests/helpers/mod.rs` so they can be reused.

### P1: Extract `insert_mr()` to shared helpers

**Source:** `tests/issue_mr_integration.rs:558-581`

Add to `tests/helpers/mod.rs`:
```rust
pub async fn insert_mr(
    pool: &PgPool,
    project_id: Uuid,
    author_id: Uuid,
    source_branch: &str,
    target_branch: &str,
    number: i32,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO merge_requests (id, project_id, number, author_id, source_branch, target_branch, title, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'Test MR', 'open')"
    )
    .bind(id).bind(project_id).bind(number).bind(author_id)
    .bind(source_branch).bind(target_branch)
    .execute(pool).await.unwrap();

    sqlx::query("UPDATE projects SET next_mr_number = $1 WHERE id = $2")
        .bind(number + 1).bind(project_id)
        .execute(pool).await.unwrap();
    id
}
```

The existing callers in `issue_mr_integration.rs` can keep their local version or call the shared one. The shared version adds `source_branch`/`target_branch` params instead of hardcoding `'feat'`/`'main'`.

### P2: Add `insert_branch_protection()` helper

New helper in `tests/helpers/mod.rs`:
```rust
pub async fn insert_branch_protection(
    pool: &PgPool,
    project_id: Uuid,
    pattern: &str,
    required_approvals: i32,
    merge_methods: &[&str],
    required_checks: &[&str],
    require_up_to_date: bool,
    allow_admin_bypass: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO branch_protection_rules
         (id, project_id, pattern, required_approvals, merge_methods, required_checks, require_up_to_date, allow_admin_bypass)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
    .bind(id).bind(project_id).bind(pattern)
    .bind(required_approvals)
    .bind(merge_methods).bind(required_checks)
    .bind(require_up_to_date).bind(allow_admin_bypass)
    .execute(pool).await.unwrap();
    id
}
```

### P3: Reuse existing `insert_pipeline()` pattern

`insert_pipeline()` already exists in `pipeline_integration.rs:34-57`. No need to share it — just duplicate the 10-line function in the new test file (or inline the INSERT).

### P4: Add `get_admin_user_id()` to shared helpers

Several tests need the admin user's UUID. Add:
```rust
pub async fn admin_user_id(pool: &PgPool) -> Uuid {
    let row: (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(pool).await.unwrap();
    row.0
}
```

---

## T2: `enforce_merge_gates()` Integration Tests

**File:** `tests/merge_gates_integration.rs` (new file)

All tests use the pattern: create project → insert branch protection rule → insert MR → POST merge → assert 400 with specific error message. No real git repo needed for rejection tests (gates fail before reaching git operations).

### Test Table

| # | Test name | Gate tested | Setup | Expected |
|---|---|---|---|---|
| 1 | `merge_blocked_wrong_method` | Merge method | Rule: `merge_methods: [squash]`, POST merge with default method (`merge`) | 400, body mentions "merge method" |
| 2 | `merge_allowed_correct_method` | Merge method (positive) | Rule: `merge_methods: [squash]`, real git repo, POST merge with `method: squash` | 200 (requires git repo) |
| 3 | `merge_blocked_insufficient_approvals` | Required approvals | Rule: `required_approvals: 2`, MR has 0 reviews | 400, body mentions "approval" |
| 4 | `merge_allowed_with_approvals` | Approvals (positive) | Rule: `required_approvals: 1`, insert 1 approve review, real git repo | 200 |
| 5 | `merge_blocked_no_ci_success` | Required CI | Rule: `required_checks: [ci]`, no pipeline exists | 400, body mentions "CI" or "pipeline" |
| 6 | `merge_blocked_ci_failed` | Required CI | Rule: `required_checks: [ci]`, insert pipeline with `status: failure` | 400 |
| 7 | `merge_allowed_ci_success` | CI (positive) | Rule: `required_checks: [ci]`, insert pipeline with `status: success`, real git repo | 200 |
| 8 | `merge_admin_bypass` | Admin bypass | Rule: `allow_admin_bypass: true`, `required_approvals: 1`, admin user merges (0 approvals), real git repo | 200 |
| 9 | `merge_admin_no_bypass` | Admin bypass disabled | Rule: `allow_admin_bypass: false`, `required_approvals: 1`, admin user | 400 |
| 10 | `merge_no_protection_passes` | No rule | No rule for the target branch | 200 (requires git repo) |

### Git Repo Setup (for tests that need successful merge)

Tests 2, 4, 7, 8, 10 need a real git repo with a source branch diverged from target:

```rust
// Create bare repo and working copy
let (bare_dir, bare_path) = helpers::create_bare_repo();
let (work_dir, work_path) = helpers::create_working_copy(&bare_path);

// Create a feature branch with a commit
helpers::git_cmd(&work_path, &["checkout", "-b", "feat"]);
std::fs::write(work_path.join("feature.txt"), "new feature").unwrap();
helpers::git_cmd(&work_path, &["add", "."]);
helpers::git_cmd(&work_path, &["commit", "-m", "add feature"]);
helpers::git_cmd(&work_path, &["push", "origin", "feat"]);

// Point the project's repo_path to the bare repo
sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
    .bind(bare_path.to_str().unwrap())
    .bind(project_id)
    .execute(&pool).await.unwrap();
```

The MR must have `source_branch: "feat"`, `target_branch: "main"` and `head_sha` matching the latest commit on `feat`.

### Rejection-Only Tests (no git repo needed)

Tests 1, 3, 5, 6, 9 only verify that `enforce_merge_gates()` rejects the merge before reaching git operations. These are simpler — just insert DB rows and POST merge:

```rust
// Insert protection rule requiring squash only
let _rule_id = helpers::insert_branch_protection(
    &pool, project_id, "main",
    0, &["squash"], &[], false, false,
).await;

// Insert MR
let admin_id = helpers::admin_user_id(&pool).await;
let mr_id = helpers::insert_mr(&pool, project_id, admin_id, "feat", "main", 1).await;

// Attempt merge with default method (merge, not squash)
let (status, body) = helpers::post_json(
    &app, &admin_token,
    &format!("/api/projects/{project_id}/merge-requests/1/merge"),
    serde_json::json!({}),
).await;
assert_eq!(status, StatusCode::BAD_REQUEST);
assert!(body["error"].as_str().unwrap().contains("merge method"), "should mention merge method");
```

**Important:** The merge handler checks `status == 'open'` before gates, and the MR row from `insert_mr` has status `'open'`. But it also calls `has_permission_scoped` first — admin_token has all permissions so this passes.

However, the handler also fetches `head_sha` from the MR and uses it. If `head_sha` is NULL (from our simple INSERT), some gates may behave differently. Need to check if the merge handler tolerates NULL head_sha for the rejection path. If not, add `head_sha = 'abc123'` to the INSERT.

---

## T3: Auto-Merge Endpoint Integration Tests

**File:** `tests/merge_gates_integration.rs` (same file as T2, separate section)

### Test Table

| # | Test name | Endpoint | Setup | Expected |
|---|---|---|---|---|
| 1 | `enable_auto_merge` | PUT `.../auto-merge` | Open MR (via insert_mr) | 200, DB shows `auto_merge=true` |
| 2 | `disable_auto_merge` | DELETE `.../auto-merge` | Open MR with auto_merge enabled | 200, DB shows `auto_merge=false` |
| 3 | `enable_auto_merge_closed_mr` | PUT | Closed MR | 404 (WHERE status='open' matches nothing) |
| 4 | `auto_merge_requires_write` | PUT | Viewer role user | 403 |
| 5 | `auto_merge_with_method` | PUT | Body: `{"merge_method": "squash"}` | 200, DB shows `auto_merge_method='squash'` |

### Setup

Auto-merge endpoints only modify DB columns — no git repo needed:

```rust
// Create project + MR
let admin_id = helpers::admin_user_id(&pool).await;
let mr_id = helpers::insert_mr(&pool, project_id, admin_id, "feat", "main", 1).await;

// Enable auto-merge
let (status, _) = helpers::put_json(
    &app, &admin_token,
    &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
    serde_json::json!({}),
).await;
assert_eq!(status, StatusCode::OK);

// Verify DB
let row: (bool,) = sqlx::query_as(
    "SELECT auto_merge FROM merge_requests WHERE id = $1"
).bind(mr_id).fetch_one(&pool).await.unwrap();
assert!(row.0, "auto_merge should be true");
```

---

## T9: Rate Limit Boundary Integration Tests

**File:** `tests/auth_integration.rs` (add to existing file)

### Test Table

| # | Test name | Boundary | Setup | Expected |
|---|---|---|---|---|
| 1 | `rate_limit_at_threshold_minus_one` | count=9 (below 10 limit) | Pre-set Valkey key to 9, make 1 login attempt | Login proceeds (may be 200 or 401 depending on password, NOT 429) |
| 2 | `rate_limit_at_threshold` | count=10 (at limit) | Pre-set to 10, make 1 attempt | 429 (count becomes 11, exceeds 10) |
| 3 | `rate_limit_window_expiry` | TTL expires | Pre-set to 10 with TTL=1, sleep 1.1s, make attempt | Login proceeds (key expired, counter reset) |

### Setup Pattern

Same pattern as existing `login_rate_limited` test — pre-seed the Valkey counter:

```rust
// Test 1: At threshold minus one (should NOT rate limit)
let rate_key = format!("rate:login:{unique_name}");
let _: () = state.valkey.set(&rate_key, 9i64, None, None, false).await.unwrap();
let _: () = state.valkey.expire(&rate_key, 300, None).await.unwrap();

// Make one attempt — INCR takes count from 9 to 10, which is == max (not >)
// check_rate uses `count > max_attempts`, so 10 > 10 is false → allowed
let (status, _) = helpers::post_json(&app, "", "/api/auth/login",
    serde_json::json!({ "name": &unique_name, "password": "testpass123" }),
).await;
// Should be 200 (correct password) or 401 (wrong password), NOT 429
assert_ne!(status, StatusCode::TOO_MANY_REQUESTS,
    "count at threshold should not trigger rate limit");
```

**Note on `check_rate` boundary:** The function uses `count > max_attempts`, not `>=`. So at count == max_attempts (10), the request is allowed. At count == 11, it's blocked. This means:
- Pre-set to 9 → INCR to 10 → `10 > 10` = false → allowed
- Pre-set to 10 → INCR to 11 → `11 > 10` = true → blocked (429)

For TTL expiry test, use a 1-second TTL and `tokio::time::sleep(Duration::from_millis(1100))` — this is acceptable since we're testing the actual Valkey TTL behavior, not waiting for application-level async.

---

## Implementation Order

| Step | What | Effort | Depends on |
|---|---|---|---|
| P1 | Extract `insert_mr()` to helpers | Small | — |
| P2 | Add `insert_branch_protection()` helper | Small | — |
| P4 | Add `admin_user_id()` helper | Trivial | — |
| T9 | Rate limit boundary tests (3 tests) | Small | P1 (not needed, just Valkey) |
| T3 | Auto-merge endpoint tests (5 tests) | Small | P1, P4 |
| T2 rejection | Merge gate rejection tests (6 tests) | Medium | P1, P2, P4 |
| T2 positive | Merge gate success tests (4 tests) | Medium | P1, P2, P4 + git repo setup |

**Total: ~18 new tests, ~3 new helpers, 1 new file + additions to existing file.**

## Implementation Status

- [x] P1: `insert_mr()` added to `tests/helpers/mod.rs`
- [x] P2: `insert_branch_protection()` added to `tests/helpers/mod.rs`
- [x] P3: `insert_pipeline()` added to `tests/helpers/mod.rs`
- [x] P4: `admin_user_id()` added to `tests/helpers/mod.rs`
- [x] T2: Merge gate tests — `tests/merge_gates_integration.rs` (8 tests: 5 rejection, 3 success with git repo)
- [x] T3: Auto-merge tests — same file (5 tests: enable, disable, closed MR, method, permissions)
- [x] T9: Rate limit boundary tests — added to `tests/auth_integration.rs` (2 new tests: threshold-1, window expiry)

### Verification

After all tests written:
```bash
just test-bin merge_gates_integration       # new file
just test-bin auth_integration rate_limit    # new rate limit tests
```

Then full suite before declaring done:
```bash
just ci-full
```
