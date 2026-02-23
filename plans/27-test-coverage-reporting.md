# Plan 27 — Three-Tier Test Coverage Reporting

## Context

The platform has 442 unit tests, ~10 integration test files, and 40 E2E tests, but no coverage tracking. A single combined coverage number is misleading — if E2E tests cover 60% of lines, it masks that unit tests only cover 30%, meaning that code is only validated through slow, fragile integration paths. Separating coverage by tier makes the testing pyramid visible and actionable.

**Goal**: independent coverage reports for unit, integration, and E2E tiers with CI enforcement on unit coverage and dashboards for all three.

---

## 1. Tooling: cargo-llvm-cov

`cargo-llvm-cov` is the standard for Rust coverage. It wraps LLVM's source-based instrumentation (not debug-info heuristics like tarpaulin), producing accurate line/branch/region data.

### Install

```bash
# Local
cargo install cargo-llvm-cov --locked

# CI (GitHub Actions)
- uses: taiki-e/install-action@cargo-llvm-cov
```

### Why not tarpaulin

- tarpaulin uses ptrace/debug-info → inaccurate on async code, misses inlined functions
- llvm-cov uses compiler instrumentation → accurate even with heavy async/await, monomorphization
- llvm-cov supports branch coverage natively
- llvm-cov is faster on large codebases (single instrumented build vs ptrace per test)

---

## 2. justfile Commands

Add six new commands to the justfile:

```just
# Coverage — per-tier
cov-unit:
    cargo llvm-cov nextest --lib --lcov --output-path coverage-unit.lcov

cov-integration:
    cargo llvm-cov nextest --test '*_integration' --lcov --output-path coverage-integration.lcov

cov-e2e:
    cargo llvm-cov nextest --test 'e2e_*' --run-ignored ignored-only --test-threads 2 --lcov --output-path coverage-e2e.lcov

# Coverage — combined (all tiers merged)
cov-all:
    cargo llvm-cov nextest --lcov --output-path coverage-all.lcov

# HTML report (local dev)
cov-html:
    cargo llvm-cov nextest --lib --html --output-dir coverage-html
    @echo "Open coverage-html/index.html"

# Summary table (quick terminal check)
cov-summary:
    @echo "=== Unit ===" && cargo llvm-cov nextest --lib --summary-only 2>&1 | tail -3
    @echo "=== Integration ===" && cargo llvm-cov nextest --test '*_integration' --summary-only 2>&1 | tail -3
```

### Ignoring generated/vendor code

Create `llvm-cov-ignore.txt` (or use `--ignore-filename-regex`):

```just
cov-unit:
    cargo llvm-cov nextest --lib --lcov --output-path coverage-unit.lcov \
        --ignore-filename-regex '(proto\.rs|ui\.rs|tests/)'
```

Exclude from coverage measurement:
- `src/observe/proto.rs` — generated protobuf types
- `src/ui.rs` — rust-embed static file serving
- `tests/` — test code itself
- `src/main.rs` — bootstrap/wiring (tested via E2E)

---

## 3. CI Pipeline Changes

### New job: `coverage` (runs after `test-unit` passes)

```yaml
coverage:
  name: Coverage
  runs-on: ubuntu-latest
  needs: [test-unit]
  services:
    postgres:
      image: postgres:17
      env:
        POSTGRES_USER: platform
        POSTGRES_PASSWORD: dev
        POSTGRES_DB: platform_dev
      ports: ['5432:5432']
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: llvm-tools-preview
    - uses: taiki-e/install-action@cargo-llvm-cov
    - uses: taiki-e/install-action@cargo-nextest

    # Unit coverage (no DB needed)
    - name: Unit coverage
      run: cargo llvm-cov nextest --lib --lcov --output-path coverage-unit.lcov
      env:
        SQLX_OFFLINE: 'true'

    # Integration coverage (needs Postgres)
    - name: Run migrations
      run: cargo run -- migrate
      env:
        DATABASE_URL: postgres://platform:dev@localhost:5432/platform_dev
    - name: Integration coverage
      run: cargo llvm-cov nextest --test '*_integration' --lcov --output-path coverage-integration.lcov
      env:
        DATABASE_URL: postgres://platform:dev@localhost:5432/platform_dev
        SQLX_OFFLINE: 'true'

    # Upload to Codecov with flags
    - uses: codecov/codecov-action@v4
      with:
        files: coverage-unit.lcov
        flags: unit
        token: ${{ secrets.CODECOV_TOKEN }}
    - uses: codecov/codecov-action@v4
      with:
        files: coverage-integration.lcov
        flags: integration
        token: ${{ secrets.CODECOV_TOKEN }}
```

E2E coverage is not run in CI (requires Kind cluster, too slow for every PR). Run it manually or in a nightly job.

### Nightly job for E2E coverage (optional)

```yaml
name: Nightly Coverage
on:
  schedule:
    - cron: '0 4 * * *'  # 4am UTC daily
jobs:
  e2e-coverage:
    runs-on: ubuntu-latest
    steps:
      # ... kind cluster setup, migrations, etc.
      - name: E2E coverage
        run: cargo llvm-cov nextest --test 'e2e_*' --run-ignored ignored-only --test-threads 2 --lcov --output-path coverage-e2e.lcov
      - uses: codecov/codecov-action@v4
        with:
          files: coverage-e2e.lcov
          flags: e2e
          token: ${{ secrets.CODECOV_TOKEN }}
```

---

## 4. Codecov Configuration

### `codecov.yml`

```yaml
codecov:
  require_ci_to_pass: true

coverage:
  status:
    project:
      default:
        target: auto
        threshold: 2%    # allow 2% drop without failing
      unit:
        flags: [unit]
        target: 60%      # start here, ratchet up over time
        threshold: 1%
      integration:
        flags: [integration]
        informational: true   # track but don't gate
      e2e:
        flags: [e2e]
        informational: true
    patch:
      default:
        target: 70%      # new code in PRs must have 70% coverage
        threshold: 5%
        flags: [unit]

flags:
  unit:
    paths:
      - src/
    carryforward: true
  integration:
    paths:
      - src/
    carryforward: true
  e2e:
    paths:
      - src/
    carryforward: true

ignore:
  - src/observe/proto.rs
  - src/ui.rs
  - src/main.rs
  - tests/
  - ui/
  - mcp/
```

### What the flags give you

On every PR, Codecov comments with a table like:

```
| Flag        | Coverage | Δ     |
|-------------|----------|-------|
| unit        | 62.3%    | +1.2% |
| integration | 45.1%    | +0.3% |
| e2e         | 58.7%    | —     |  (nightly, carryforward)
```

The **patch** check ensures new code has unit tests. The **project** check ensures overall unit coverage doesn't regress.

---

## 5. Coverage Targets & Ratchet Strategy

Don't set aspirational targets. Set achievable ones and ratchet upward.

### Initial targets (measure first, then set)

1. Run `just cov-unit` locally to get current baseline
2. Set `target` to current baseline minus 2%
3. Every month, if coverage has increased, bump the target to new baseline minus 2%

### Per-module expectations

| Module | Expected unit coverage | Notes |
|--------|----------------------|-------|
| `validation` | >90% | Pure functions, easy to test |
| `rbac` | >80% | Permission resolution, state machines |
| `pipeline/definition` | >80% | Parser, pattern matching |
| `secrets/engine` | >90% | Encrypt/decrypt round-trips |
| `auth/password` | >80% | Hashing, timing-safe compare |
| `observe/proto` | Skip | Generated types |
| `api/*` handlers | 30-50% | Tested mainly via integration/E2E |
| `deployer/*` | 30-50% | K8s interaction, tested via E2E |

### What low unit + high E2E coverage tells you

If a module has <30% unit coverage but >70% E2E coverage, it means:
- Logic is only tested through slow, flaky paths
- Failures are hard to localize (which layer broke?)
- Refactoring is risky (no fast feedback loop)

Action: extract pure logic into testable functions, add unit tests, reduce E2E dependency.

---

## 6. Local Developer Workflow

### Quick check before pushing

```bash
just cov-unit          # generates coverage-unit.lcov
just cov-html          # opens HTML report in browser
```

### Investigating uncovered lines

```bash
# Show uncovered regions in a specific file
cargo llvm-cov nextest --lib --show-missing-lines -- -E 'test(rbac)'
```

### VS Code integration

The Coverage Gutters extension reads lcov files and shows green/red gutters inline:

1. Install `ryanluker.vscode-coverage-gutters`
2. Run `just cov-unit` (generates `coverage-unit.lcov` in project root)
3. Cmd+Shift+P → "Coverage Gutters: Display Coverage"

---

## 7. .gitignore Updates

```gitignore
# Coverage artifacts
*.lcov
coverage-html/
target/llvm-cov-target/
```

---

## 8. Implementation Steps

1. **Add cargo-llvm-cov to dev toolchain** — document in README/CLAUDE.md
2. **Add justfile commands** — `cov-unit`, `cov-integration`, `cov-e2e`, `cov-all`, `cov-html`, `cov-summary`
3. **Add .gitignore entries** for coverage artifacts
4. **Run baseline measurement** — `just cov-unit` to get current numbers
5. **Create `codecov.yml`** with flags and initial targets based on baseline
6. **Add CI job** — `coverage` job in `.github/workflows/ci.yaml`
7. **Set up Codecov** — add `CODECOV_TOKEN` to repo secrets, enable Codecov GitHub App
8. **Optional: nightly E2E coverage** — separate workflow with Kind cluster
9. **Update `docs/testing.md`** — add coverage section
10. **Update `CLAUDE.md`** — add `just cov-*` commands to the commands table

---

## 9. Tradeoffs & Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Coverage tool | cargo-llvm-cov | Accurate on async Rust, supports branch coverage, fast |
| Coverage service | Codecov | Free for OSS, flag support for per-tier tracking, PR comments |
| Unit gate | Yes (target + patch) | Prevents regression, enforces tests on new code |
| Integration gate | No (informational) | Too variable, DB-dependent, flaky threshold |
| E2E in CI | Nightly only | Too slow/expensive for every PR, Kind cluster setup |
| Branch coverage | Not initially | Line coverage first, add branch later when baseline is stable |
| Per-file enforcement | No | Too granular, creates noisy PR checks. Per-flag is enough |

---

## 10. Alternative: Coveralls

If Codecov is unavailable or the free tier is insufficient:

```yaml
- uses: coverallsapp/github-action@v2
  with:
    files: coverage-unit.lcov coverage-integration.lcov
    flag-name: unit
    parallel: true
```

Coveralls supports flags similarly but the PR comment UX is less detailed than Codecov's.
