# Review: 35-git-ssh-gpg

**Date:** 2026-02-26
**Scope:** SSH key management, GPG key management, SSH git server, commit signature verification
**Overall:** PASS WITH FINDINGS

## Summary

- Solid implementation across 4 PRs: SSH key CRUD, SSH git protocol, GPG key CRUD, and commit signature verification. Well-structured code, thorough input validation, proper audit logging, good security posture on SSH command parsing.
- **1 high, 6 medium findings** — the most critical is a key_id case mismatch that prevents GPG signature verification from ever succeeding.
- **Test coverage**: 956 unit tests pass. Integration tests cover all CRUD handlers. However, the Verified/UnverifiedSigner/BadSignature signature verification paths are completely untested end-to-end.
- **Touched-line coverage (unit)**: 92% signature.rs, 97% ssh_keys.rs, 91% gpg_keys.rs, 100% config.rs. Handler files (api/ssh_keys.rs, api/gpg_keys.rs) are 0% unit but covered by integration.

## Changed Files

- `src/api/ssh_keys.rs` (new, 251 lines)
- `src/api/gpg_keys.rs` (new, 339 lines)
- `src/api/mod.rs` (modified)
- `src/git/ssh_keys.rs` (new, 247 lines)
- `src/git/gpg_keys.rs` (new, 344 lines)
- `src/git/ssh_server.rs` (new, 641 lines)
- `src/git/signature.rs` (new, 299 lines)
- `src/git/browser.rs` (modified, +280 lines)
- `src/git/smart_http.rs` (modified, refactored check_access)
- `src/git/mod.rs` (modified)
- `src/config.rs` (modified, +SSH config fields)
- `src/main.rs` (modified, SSH server spawn)
- `migrations/20260226010001_user_ssh_keys.{up,down}.sql` (new)
- `migrations/20260226163003_user_gpg_keys.{up,down}.sql` (new)
- `tests/ssh_keys_integration.rs` (new)
- `tests/gpg_keys_integration.rs` (new)
- `tests/gpg_signature_integration.rs` (new)
- `tests/ssh_server_integration.rs` (new)
- `tests/e2e_ssh.rs` (new)

## Critical & High Findings (must fix)

### R1: [HIGH] GPG key_id case mismatch breaks all signature verification

- **File:** `src/git/gpg_keys.rs:69` + `src/git/signature.rs:104` + `src/git/browser.rs:550`
- **Domain:** Security / Database
- **Description:** `parse_gpg_public_key()` stores `key_id` as uppercase hex (via `.to_uppercase()` on the fingerprint). But `extract_signing_key_id()` returns lowercase hex (via `hex::encode()`). The DB lookup `WHERE key_id = $1` is case-sensitive, so verification will **never** find a matching key. All signed commits will show `BadSignature`.
- **Risk:** GPG signature verification is completely broken for all commits. Users will never see `Verified` status.
- **Suggested fix:** Normalize the case in `extract_signing_key_id()`:
  ```rust
  issuers.first().map(|id| hex::encode(id.as_ref()).to_uppercase())
  ```

## Medium Findings (should fix)

### R2: [MEDIUM] browser.rs check_project_read returns 403 instead of 404

- **File:** `src/git/browser.rs:192`
- **Domain:** Security
- **Description:** `check_project_read()` returns `ApiError::Forbidden` when permission is denied. Per project security patterns, private resources must return **404** to avoid leaking resource existence. All browser endpoints (tree, blob, branches, commits, commit_detail) are affected.
- **Suggested fix:** Change `return Err(ApiError::Forbidden)` to `return Err(ApiError::NotFound("project".into()))`.

### R3: [MEDIUM] Dynamic sqlx::query() in production code

- **File:** `src/git/browser.rs:550`
- **Domain:** Database
- **Description:** `lookup_gpg_key()` uses `sqlx::query()` (dynamic) instead of `sqlx::query!()` (compile-time). Project convention requires compile-time queries in `src/`. The `.get()` calls can panic at runtime if column names change.
- **Suggested fix:** Convert to `sqlx::query!()` with proper typed access. Remove the `GpgKeyRow` helper struct and use the sqlx-generated anonymous row.

### R4: [MEDIUM] GPG key lookup doesn't filter can_sign or check expiration

- **File:** `src/git/browser.rs:550`
- **Domain:** Database / Security
- **Description:** The query `SELECT ... FROM user_gpg_keys WHERE key_id = $1` returns keys regardless of `can_sign` flag or expiry. An expired or non-signing key would be used for verification.
- **Suggested fix:** Add filters: `AND can_sign = true AND (expires_at IS NULL OR expires_at > now())`.

### R5: [MEDIUM] Signature cache not invalidated on GPG key deletion

- **File:** `src/api/gpg_keys.rs:270-303` + `src/git/browser.rs:486-504`
- **Domain:** Security
- **Description:** Cached verification results (1hr TTL in Valkey) are not cleared when a GPG key is deleted. Commits will continue showing as `Verified` for up to 1 hour after the signing key is removed.
- **Suggested fix:** Either (a) invalidate related cache entries on key deletion, or (b) reduce TTL to ~5 min, or (c) after cache hit, verify the key still exists in DB.

### R6: [MEDIUM] No integration test with actual GPG-signed commit

- **File:** `tests/gpg_signature_integration.rs`
- **Domain:** Tests
- **Description:** All 7 signature integration tests only exercise the `NoSignature` path (unsigned commits). The Verified, UnverifiedSigner, and BadSignature paths are completely untested end-to-end. This is the most significant test gap — the entire verification pipeline has no proof of correctness.
- **Suggested fix:** Add tests that:
  1. Generate a GPG key, register it, make a signed commit → expect `verified`
  2. Sign commit with registered key but mismatched email → expect `unverified_signer`
  3. Sign commit with unregistered key → expect `bad_signature`

### R7: [MEDIUM] GPG key max-50 limit not tested

- **File:** `tests/gpg_keys_integration.rs`
- **Domain:** Tests
- **Description:** SSH keys have an integration test for the 50-key-per-user limit, but GPG keys do not.
- **Suggested fix:** Add test similar to SSH key limit test: insert 50 rows directly, attempt 51st via API, assert 400.

## Low Findings (optional)

- **R8** [LOW]: `src/git/browser.rs:557` — `lookup_gpg_key()` swallows DB errors via `.ok()??`. DB connection failures silently produce `BadSignature` instead of surfacing the error. Consider logging the error.
- **R9** [LOW]: `src/git/ssh_server.rs:186` — Logs raw SSH command string on rejection. Could be used for log flooding. Consider truncating to 256 chars.
- **R10** [LOW]: `src/git/signature.rs:99-124` — `extract_signing_key_id()` and `verify_signature()` have no unit tests with valid inputs. Only invalid/error paths are tested.
- **R11** [LOW]: `tests/ssh_keys_integration.rs` — `TEST_ECDSA_P256_KEY` constant declared but unused.
- **R12** [LOW]: Missing unauthenticated access tests for admin SSH key list, GPG key detail, and admin GPG key list endpoints.

## Coverage — Touched Lines (Unit Tests)

| File | Lines instrumented | Lines covered | Coverage % | Notes |
|---|---|---|---|---|
| `src/git/signature.rs` | 174 | 161 | 92% | L99-124 uncovered (extract_signing_key_id, verify_signature) |
| `src/git/ssh_keys.rs` | 107 | 104 | 97% | L22-24 uncovered (From impl) |
| `src/git/gpg_keys.rs` | 156 | 143 | 91% | L20-22 uncovered (From impl) + internal edge cases |
| `src/git/ssh_server.rs` | 284 | 115 | 40% | Handler code covered by integration/E2E |
| `src/git/browser.rs` | 379 | 151 | 39% | Handler + signature verification covered by integration |
| `src/api/ssh_keys.rs` | 133 | 0 | 0% | All handler code — covered by integration tests |
| `src/api/gpg_keys.rs` | 191 | 0 | 0% | All handler code — covered by integration tests |
| `src/config.rs` | 181 | 181 | 100% | Fully covered |

**Note:** API handler files (0% unit) and SSH server handler code (40% unit) are expected to have low unit coverage — they are covered by integration and E2E test tiers. The key gaps are in `signature.rs` (L99-124: the actual crypto verification functions) which have no test at any tier exercising the success path.

## Checklist Results

| Category | Status | Notes |
|---|---|---|
| Error handling | PASS | thiserror for module errors, From impls for ApiError |
| Auth & permissions | PASS | All handlers use AuthUser, admin checks present |
| Input validation | PASS | Length checks, rate limiting, max key limits |
| Audit logging | PASS | All mutations audited |
| Tracing instrumentation | PASS | Key async fns instrumented |
| Clippy compliance | PASS | Clean under `--all-features -- -D warnings` |
| Test patterns | PASS | Correct use of helpers, sqlx::test, dynamic queries in tests |
| Migration safety | PASS | Proper UP/DOWN, indexes, FK with CASCADE |
| Private resource leakage | **FAIL** | browser.rs returns 403 not 404 (R2) |
| GPG verification correctness | **FAIL** | Case mismatch (R1), no can_sign/expiry filter (R4) |
| Cache correctness | **FAIL** | No invalidation on key delete (R5) |
