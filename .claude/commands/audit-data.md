# Skill: Database & Migration Audit — Schema, Queries & Data Integrity

**Description:** Orchestrates 5 parallel AI agents that audit the database layer: migration correctness, schema design, query patterns, index coverage, constraint completeness, and `.sqlx/` offline cache consistency. Catches schema drift, missing indexes, unsafe migrations, and data integrity gaps that the code-level audits miss.

**When to use:** After adding migrations, before a release, when queries are slow, when adding new tables/columns, or when `.sqlx/` cache errors appear in CI.

---

## Orchestrator Instructions

You are the **Database Auditor**. Your job is to:

1. Inventory all migrations and query files
2. Launch 5 parallel agents that audit different aspects of the data layer
3. Synthesize findings into a prioritized report
4. Produce a persistent `plans/data-audit-<date>.md` report

### Severity Levels

| Severity | Meaning | Action |
|---|---|---|
| **CRITICAL** | Data loss risk, irreversible migration, constraint violation in prod | Fix immediately |
| **HIGH** | Missing index on hot path, broken down migration, schema drift | Fix before release |
| **MEDIUM** | Suboptimal schema, missing constraint, inconsistent naming | Fix when touching the area |
| **LOW** | Minor naming, optional optimization, style nit | Fix only if trivial |

---

## Phase 0: Inventory

```bash
# Migration files
echo "=== Migrations ==="
ls -la migrations/ | head -60
echo "Migration count: $(ls migrations/*.sql 2>/dev/null | wc -l)"

# sqlx offline cache
echo "=== .sqlx/ cache ==="
ls .sqlx/ | wc -l
echo "Cache files:"
ls .sqlx/ | head -20

# Query patterns in src/
echo "=== Compile-time queries ==="
grep -rn 'sqlx::query!' src/ --include='*.rs' | wc -l
grep -rn 'sqlx::query_as!' src/ --include='*.rs' | wc -l
grep -rn 'sqlx::query_scalar!' src/ --include='*.rs' | wc -l

echo "=== Dynamic queries (tests) ==="
grep -rn 'sqlx::query(' tests/ --include='*.rs' | wc -l

echo "=== Tables referenced ==="
grep -roh 'FROM \w\+\|INTO \w\+\|UPDATE \w\+\|JOIN \w\+' src/ --include='*.rs' | sort -u
```

---

## Phase 1: Parallel Database Audits

Launch **all 5 agents concurrently**.

---

### Agent 1: Migration Correctness & Safety

**Scope:** All files under `migrations/`

**Read ALL migration files (both `.up.sql` and `.down.sql`), then check:**

_Migration ordering:_
- [ ] Version numbers are sequential and non-overlapping
- [ ] No duplicate version prefixes (the first segment before `_` is the version)
- [ ] Timestamps are monotonically increasing
- [ ] No gaps in sequence that suggest a missing migration

_Up migration safety:_
- [ ] `CREATE TABLE` uses `IF NOT EXISTS` where appropriate for idempotency
- [ ] `ALTER TABLE` doesn't drop columns with data (should migrate data first)
- [ ] No `DROP TABLE` without prior data migration
- [ ] `NOT NULL` columns added to existing tables have `DEFAULT` values
- [ ] Index creation uses `CONCURRENTLY` where possible (to avoid table locks)
- [ ] No raw data manipulation that could fail on large tables (long-running transactions)
- [ ] Foreign key constraints have appropriate `ON DELETE` behavior
- [ ] `CHECK` constraints are complete (no missing enum values)

_Down migration safety:_
- [ ] Every `.up.sql` has a corresponding `.down.sql`
- [ ] Down migrations are actually reversible (don't just `DROP TABLE` if up created data)
- [ ] Down migrations handle the case where the up migration partially applied
- [ ] Down migration of a column addition doesn't lose data without warning

_Schema evolution:_
- [ ] Column types are appropriate (UUID for IDs, TIMESTAMPTZ for times, TEXT vs VARCHAR)
- [ ] All tables have `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- [ ] Mutable tables have `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- [ ] Primary keys use `gen_random_uuid()`
- [ ] No `SERIAL` / `BIGSERIAL` for primary keys (should use UUID)
- [ ] Enum types use TEXT + CHECK constraints (not Postgres enums — they're hard to migrate)

**Output:** Numbered findings with migration file, line, description, and fix.

---

### Agent 2: Schema Design & Constraints

**Scope:** All `migrations/*.up.sql` (to reconstruct full schema), cross-referenced with `src/` query patterns

**Reconstruct the current schema from migrations, then check:**

_Referential integrity:_
- [ ] Every foreign key reference targets a valid table/column
- [ ] FK `ON DELETE` behavior matches business logic:
  - User deletion: CASCADE sessions/tokens, what about issues/MRs/comments?
  - Project deletion: soft-delete (`is_active`), are FKs to project_id safe?
  - Workspace deletion: what cascades?
- [ ] No orphan-prone relationships (child can exist without parent)

_Constraint completeness:_
- [ ] All enum-like columns have CHECK constraints matching Rust enum variants
- [ ] Numeric fields have range constraints where applicable (e.g., `weight >= 0 AND weight <= 100`)
- [ ] String fields have length constraints where applicable
- [ ] URL fields validated at DB level (or only at app level — document the choice)
- [ ] Boolean fields have defaults

_Uniqueness:_
- [ ] Natural keys have UNIQUE constraints (e.g., `(project_id, number)` for issues)
- [ ] No duplicate data possible for fields that should be unique (user email, project slug, etc.)
- [ ] Composite unique constraints where needed

_Soft-delete consistency:_
- [ ] All queries on soft-deletable tables include `AND is_active = true`
- [ ] Unique constraints account for soft-delete (partial unique index on active records only?)
- [ ] Are there soft-deleted records that break unique constraints when restored?

_Naming conventions:_
- [ ] Table names: snake_case, plural
- [ ] Column names: snake_case
- [ ] FK columns: `{referenced_table_singular}_id`
- [ ] Index names: consistent pattern
- [ ] Constraint names: descriptive

**Output:** Numbered findings with table, column, description, and fix.

---

### Agent 3: Query Patterns & Performance

**Scope:** All `sqlx::query*!()` calls in `src/` and all `sqlx::query()` calls in `tests/`

**Read ALL query call sites, then check:**

_N+1 query patterns:_
- [ ] List endpoints that fetch a collection then loop to fetch related data (N+1)
- [ ] Are there JOINs where they should be? (Fetch related data in one query)
- [ ] Are there subqueries that could be JOINs?

_Index coverage:_
- [ ] Every `WHERE` clause column has an index (or is part of a composite index)
- [ ] Every `ORDER BY` column has an index (especially with `LIMIT`)
- [ ] Every `JOIN` column has an index on the FK side
- [ ] Composite indexes match actual query patterns (column order matters)
- [ ] Partial indexes used where appropriate (e.g., `WHERE is_active = true`)
- [ ] No full table scans on large tables

_Query efficiency:_
- [ ] `SELECT *` avoided — only select needed columns
- [ ] `COUNT(*)` queries use efficient paths (index-only scan possible?)
- [ ] Pagination uses `LIMIT/OFFSET` with an index on the sort column
- [ ] No unbounded queries (missing `LIMIT`)
- [ ] `IN (...)` clauses have bounded size
- [ ] `LIKE '%...%'` patterns — will they need full-text search at scale?

_Transaction safety:_
- [ ] Multi-statement mutations use transactions
- [ ] Transaction scope is minimal (no long-running transactions)
- [ ] Deadlock-prone query ordering? (Always acquire locks in consistent order)

_Type safety:_
- [ ] `sqlx::query!` (compile-time) used in `src/`, `sqlx::query` (dynamic) only in `tests/`
- [ ] Type casts are safe (no truncation, no loss of precision)
- [ ] NULL handling correct (Option<T> for nullable columns)
- [ ] UUID types match between Rust and Postgres

**Output:** Numbered findings with file:line, query description, and fix.

---

### Agent 4: `.sqlx/` Offline Cache & CI Integrity

**Scope:** `.sqlx/` directory, `Cargo.toml` (sqlx features), Justfile (`db-*` recipes), CI workflow

**Check:**

_Cache consistency:_
- [ ] Number of `.json` files in `.sqlx/` matches number of `sqlx::query*!()` calls in `src/`
- [ ] No orphaned cache files (query was removed but cache file remains)
- [ ] No missing cache files (query exists but cache file missing)
- [ ] Cache file hashes match current query text (run `just db-check` if possible)

_Offline build:_
- [ ] `SQLX_OFFLINE=true` is used in CI build steps
- [ ] `just build` sets `SQLX_OFFLINE=true`
- [ ] `.sqlx/` is committed to git (not gitignored)

_Migration workflow:_
- [ ] `just db-migrate && just db-prepare` documented as required after SQL changes
- [ ] `just db-check` exists and verifies cache freshness
- [ ] CI runs `just db-check` or equivalent

_sqlx configuration:_
- [ ] `sqlx` features in Cargo.toml include necessary type support (uuid, chrono, ipnetwork, etc.)
- [ ] Database URL format matches what sqlx expects
- [ ] Connection pool settings reasonable (max connections, timeout)

**Output:** Numbered findings.

---

### Agent 5: Data Integrity & Business Rules in SQL

**Scope:** All migrations + all queries in `src/` that enforce business rules

**Check business rules enforced at DB level:**

_State machines:_
- [ ] Pipeline status transitions: does the CHECK constraint match `PipelineStatus::can_transition_to()`?
- [ ] Deployment status transitions: CHECK constraint matches Rust enum?
- [ ] Session status transitions: CHECK constraint matches Rust enum?
- [ ] Any status enum in Rust that's missing a CHECK constraint in SQL?

_Counters and sequences:_
- [ ] `next_issue_number` / `next_mr_number` — atomic increment via `UPDATE ... RETURNING`?
- [ ] Race condition: can two concurrent requests get the same number?
- [ ] Counter reset: what happens on project re-creation?

_Audit trail:_
- [ ] `audit_log` table captures all required fields
- [ ] Audit entries are append-only (no UPDATE/DELETE on audit_log)
- [ ] Is there an index on `audit_log` for common queries (by actor, by resource, by time)?

_Timestamp consistency:_
- [ ] All `created_at` / `updated_at` use `DEFAULT now()` (not application-supplied)
- [ ] `updated_at` actually updated on mutations (trigger or application logic)?
- [ ] Timezone consistency: all TIMESTAMPTZ, no naive timestamps

_Permission data:_
- [ ] Role/permission tables have proper constraints
- [ ] Delegation chains can't create cycles (or cycles are handled)
- [ ] Permission cache TTL matches what CLAUDE.md documents

**Output:** Numbered findings.

---

## Phase 2: Synthesis

Deduplicate, prioritize, categorize:
- **Data loss risks** — migrations that could lose data
- **Performance** — missing indexes, N+1 patterns
- **Integrity gaps** — missing constraints, orphan-prone FKs
- **Schema design** — naming, types, normalization
- **Tooling** — `.sqlx/` cache, migration workflow

Number findings DB1, DB2, DB3...

---

## Phase 3: Write Report

Persist as `plans/data-audit-<YYYY-MM-DD>.md`.

```markdown
# Database & Migration Audit Report

**Date:** <today>
**Scope:** N migrations, N tables, N compile-time queries, .sqlx/ cache
**Auditor:** Claude Code (automated)

## Executive Summary
- Database health: GOOD / NEEDS ATTENTION / CRITICAL ISSUES
- Findings: X critical, Y high, Z medium, W low

## Schema Overview
| Table | Columns | Indexes | FKs | CHECK | Status |
|---|---|---|---|---|---|
| users | N | N | N | N | ✓/⚠ |
| ... | ... | ... | ... | ... | ... |

## Critical & High Findings
### DB1: [SEVERITY] {title}
...

## Index Coverage Matrix
| Query Pattern | Table | WHERE/JOIN Columns | Index? | Finding |
|---|---|---|---|---|
| List issues by project | issues | project_id, is_active | ✓ | — |
| ... | ... | ... | ✗ | DB5 |

## Recommended Action Plan
...
```

---

## Phase 4: Summary to User

1. Database health (one sentence)
2. Finding counts
3. Top 3 riskiest findings
4. Missing indexes summary
5. Migration safety summary
6. Path to report
