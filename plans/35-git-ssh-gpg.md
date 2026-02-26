# Plan: Full Git Server — SSH Protocol + GPG Commit Signing

## Context

The platform currently supports git over HTTP (smart protocol) with Basic Auth (password or API token). Users want GitHub-like SSH clone/push support and GPG commit signature verification ("Verified" badges). This plan adds:

1. **SSH key management** — CRUD API for user SSH public keys
2. **SSH git protocol server** — `russh`-based server on a configurable port
3. **GPG key management** — CRUD API for user GPG public keys
4. **Commit signature verification** — verify GPG signatures in the commit browser API

SSH server is **opt-in** via `PLATFORM_SSH_LISTEN`. GPG keys are optional per user, but if uploaded, at least one UID email must match the user's platform email.

---

## PR 1: SSH Key Management (API + Database)

### Migration

**`migrations/20260226010001_user_ssh_keys.up.sql`**:
```sql
CREATE TABLE user_ssh_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    algorithm       TEXT NOT NULL,           -- ssh-ed25519, ssh-rsa, ecdsa-sha2-nistp256, etc.
    fingerprint     TEXT NOT NULL UNIQUE,     -- SHA256:... globally unique (one key = one user, like GitHub)
    public_key_openssh TEXT NOT NULL,         -- canonical re-serialized OpenSSH format (not raw user input)
    last_used_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_user_ssh_keys_user ON user_ssh_keys(user_id);
```

**`migrations/20260226010001_user_ssh_keys.down.sql`**:
```sql
DROP TABLE IF EXISTS user_ssh_keys;
```

**Schema notes:**
- `fingerprint UNIQUE` is global — same SSH key cannot belong to two users (matches GitHub behavior).
- Removed redundant `UNIQUE (user_id, fingerprint)` (global UNIQUE already implies per-user uniqueness).
- Removed redundant `idx_user_ssh_keys_fingerprint` index (UNIQUE constraint already creates one).
- Removed `public_key BYTEA` column — the OpenSSH text format is the single source of truth. Raw bytes can be re-derived by the `ssh-key` crate at read time if needed.
- Keys are immutable after creation (only `last_used_at` changes via SSH auth), so no `updated_at` column needed.

### New dependency

`Cargo.toml`: `ssh-key = { version = "0.6", features = ["ed25519", "rsa", "ecdsa"] }` — pure Rust (RustCrypto), no openssl. **Must use 0.6** (not 0.7) because `russh 0.49` depends on `ssh-key 0.6`; mixing 0.6 + 0.7 causes duplicate types. Version 0.7 only exists as release candidates.

### New files

**`src/git/ssh_keys.rs`** (~150 LOC) — parsing + validation:
- `parse_ssh_public_key(input: &str) -> Result<ParsedSshKey, SshKeyError>` — parse OpenSSH format, compute SHA-256 fingerprint, validate algorithm, re-serialize to canonical OpenSSH form
- Allowed: `ssh-ed25519`, `ecdsa-sha2-nistp256`, `ecdsa-sha2-nistp384`, `ssh-rsa` (min 2048-bit)
- Store the **re-serialized canonical** OpenSSH format (via `key.to_openssh()`), not raw user input — prevents trailing comments, extra whitespace, or injected content
- Error enum:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum SshKeyError {
      #[error("invalid SSH public key format")]
      InvalidFormat,
      #[error("unsupported algorithm: {0}")]
      UnsupportedAlgorithm(String),
      #[error("RSA key too short: {0} bits (minimum 2048)")]
      RsaKeyTooShort(u32),
      #[error("failed to compute fingerprint")]
      FingerprintError,
  }
  impl From<SshKeyError> for ApiError {
      fn from(err: SshKeyError) -> Self { ApiError::BadRequest(err.to_string()) }
  }
  ```

**`src/api/ssh_keys.rs`** (~200 LOC) — CRUD handlers following `src/api/passkeys.rs` pattern:

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/users/me/ssh-keys` | AuthUser | List current user's SSH keys |
| POST | `/api/users/me/ssh-keys` | AuthUser | Add SSH public key |
| DELETE | `/api/users/me/ssh-keys/{id}` | AuthUser | Delete SSH key (own only) |
| GET | `/api/admin/users/{user_id}/ssh-keys` | AdminUsers (scoped) | List any user's keys |

**Note:** Uses `/api/users/me/` prefix for consistency with existing `user_keys.rs` (`/api/users/me/provider-keys`).

- Validation: name 1-255 chars, public_key 20-16384 chars, parse via `ssh_keys::parse_ssh_public_key()`
- Max 50 keys per user (count check before insert)
- Fingerprint uniqueness enforced by DB UNIQUE constraint → `ApiError::Conflict`
- **Rate limiting**: `check_rate(&state.valkey, "ssh_key_add", &auth.user_id.to_string(), 20, 300)` on POST
- Delete handler WHERE clause must include `AND user_id = auth.user_id` (scope to own keys)
- Admin endpoint must use `has_permission_scoped` (not `has_permission`) — reuse `require_admin` helper from `admin.rs`
- Audit: `ssh_key.add`, `ssh_key.delete`
- All response types derive `Serialize, TS` with `#[ts(export)]`

### Modified files

| File | Change |
|------|--------|
| `src/git/mod.rs` | Add `pub mod ssh_keys;` |
| `src/api/mod.rs` | Add `pub mod ssh_keys;` + `.merge(ssh_keys::router())` |
| `Cargo.toml` | Add `ssh-key` |

### Tests

#### Tests to write FIRST (before implementation)

**Unit tests — `src/git/ssh_keys.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_parse_ed25519_key` | Valid ed25519 key parses correctly, returns algorithm + fingerprint | Unit |
| `test_parse_rsa_key_4096` | Valid RSA 4096-bit key parses | Unit |
| `test_parse_rsa_key_2048_minimum` | RSA 2048-bit key accepted (minimum) | Unit |
| `test_parse_rsa_key_1024_rejected` | RSA 1024-bit key rejected with `RsaKeyTooShort` | Unit |
| `test_parse_ecdsa_nistp256` | Valid ECDSA P-256 key parses | Unit |
| `test_parse_ecdsa_nistp384` | Valid ECDSA P-384 key parses | Unit |
| `test_parse_key_with_comment` | Key with trailing comment parses (comment stripped) | Unit |
| `test_parse_key_without_comment` | Key without comment parses | Unit |
| `test_parse_dsa_key_rejected` | DSA keys rejected with `UnsupportedAlgorithm` | Unit |
| `test_parse_empty_string` | Empty input returns `InvalidFormat` | Unit |
| `test_parse_garbage_input` | Non-base64 garbage returns error | Unit |
| `test_parse_truncated_key` | Truncated base64 returns error | Unit |
| `test_fingerprint_deterministic` | Same key → same fingerprint | Unit |
| `test_fingerprint_format` | Fingerprint starts with `SHA256:` | Unit |
| `test_different_keys_different_fingerprints` | Two different keys → different fingerprints | Unit |

**Integration tests — `tests/ssh_keys_integration.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_add_ssh_key_success` | POST returns 201 with correct fields | Integration |
| `test_list_ssh_keys_empty` | GET returns empty array for fresh user | Integration |
| `test_list_ssh_keys_with_data` | After add, list returns key | Integration |
| `test_delete_ssh_key_success` | DELETE returns 200, key gone | Integration |
| `test_delete_ssh_key_not_found` | DELETE nonexistent returns 404 | Integration |
| `test_delete_other_users_key_returns_not_found` | DELETE another user's key returns 404 | Integration |
| `test_add_duplicate_fingerprint_returns_conflict` | Same key twice returns 409 | Integration |
| `test_add_ssh_key_invalid_key_returns_400` | Garbage public_key returns 400 | Integration |
| `test_add_ssh_key_name_too_long_returns_400` | 256-char name returns 400 | Integration |
| `test_add_ssh_key_empty_name_returns_400` | Empty name returns 400 | Integration |
| `test_add_ssh_key_public_key_too_short_returns_400` | Very short key returns 400 | Integration |
| `test_add_ssh_key_public_key_too_long_returns_400` | >16384-char key returns 400 | Integration |
| `test_add_ssh_key_max_50_limit` | 51st key rejected | Integration |
| `test_list_ssh_keys_unauthenticated` | GET without token returns 401 | Integration |
| `test_add_ssh_key_unauthenticated` | POST without token returns 401 | Integration |
| `test_delete_ssh_key_unauthenticated` | DELETE without token returns 401 | Integration |
| `test_admin_list_user_ssh_keys` | Admin endpoint returns user's keys | Integration |
| `test_admin_list_ssh_keys_non_admin_denied` | Non-admin denied | Integration |
| `test_add_ssh_key_creates_audit_log` | Audit log has `ssh_key.add` | Integration |
| `test_delete_ssh_key_creates_audit_log` | Audit log has `ssh_key.delete` | Integration |
| `test_list_ssh_keys_only_own_keys` | Each user sees only their keys | Integration |
| `test_add_ssh_key_rsa_4096` | RSA 4096 succeeds via API | Integration |
| `test_add_ssh_key_rsa_1024_rejected` | RSA 1024 rejected via API | Integration |

**Total: 15 unit + 23 integration = 38 tests**

#### Existing tests to UPDATE

None — PR 1 changes are additive (new module + merge).

#### Branch coverage checklist

| Branch/Path | Test |
|---|---|
| Parse ed25519/RSA/ECDSA (valid) | `test_parse_ed25519_key`, `_rsa_4096`, `_ecdsa_*` |
| RSA < 2048 rejected | `test_parse_rsa_key_1024_rejected` |
| DSA rejected | `test_parse_dsa_key_rejected` |
| Empty/garbage/truncated input | `test_parse_empty_string`, `_garbage_input`, `_truncated_key` |
| `SshKeyError → BadRequest` | `test_add_ssh_key_invalid_key_returns_400` |
| Name validation (empty, too long) | `test_add_ssh_key_empty_name_*`, `_name_too_long_*` |
| Key length validation | `test_add_ssh_key_public_key_too_short_*`, `_too_long_*` |
| Max 50 keys limit | `test_add_ssh_key_max_50_limit` |
| Fingerprint UNIQUE → Conflict | `test_add_duplicate_fingerprint_returns_conflict` |
| Delete own key / other user's key | `test_delete_ssh_key_success`, `_other_users_key_*` |
| Auth required (401) | `test_*_unauthenticated` |
| Admin permission check | `test_admin_list_*` |
| Audit logging | `test_*_creates_audit_log` |

#### Tests NOT needed

| Excluded | Why |
|---|---|
| E2E tests for SSH key CRUD | CRUD is DB+API only — no K8s/git |
| Fingerprint collision tests | SHA-256 collision infeasible |
| `last_used_at` update | Updated by SSH server (PR 2) |

---

## PR 2: SSH Git Protocol Server

### New dependencies

```toml
russh = { version = "0.49", default-features = false, features = ["flate2"] }
russh-keys = { version = "0.49", default-features = false }
```

**Version rationale**: `russh 0.49` uses `rand 0.8` / `rand_core 0.6`; platform uses `rand 0.10` / `rand_core 0.9`. These are incompatible at the trait level. **Never pass RNG instances between russh and platform code.** Cargo compiles both versions (binary bloat but functional). If upgrading to `russh 0.57` (latest), it uses `rand 0.9` — still not `rand 0.10` but closer. Pin to 0.49 for now; evaluate 0.57 if API is needed.

**Must verify**: `cargo deny check` passes after adding. `russh 0.49.2` does NOT depend on openssl, but verify with `cargo tree -i openssl-sys` after adding.

### Config changes

**`src/config.rs`** — add fields:
```rust
pub ssh_listen: Option<String>,           // PLATFORM_SSH_LISTEN (None = disabled)
pub ssh_host_key_path: String,            // PLATFORM_SSH_HOST_KEY_PATH (default: /data/ssh_host_ed25519_key)
```

**Also update `Config::test_default()`** — add:
```rust
ssh_listen: None,
ssh_host_key_path: "/tmp/test_ssh_host_key".into(),
```
Without this, every test fails to compile.

### Refactor `src/git/smart_http.rs`

Extract shared RBAC logic for reuse by SSH:
```rust
pub async fn check_access_for_user(
    state: &AppState,
    git_user: &GitUser,
    project: &ResolvedProject,
    is_read: bool,
) -> Result<(), ApiError>
```
**Returns `Result<(), ApiError>`** — NOT `Result<Option<GitUser>>`. The caller already has the `GitUser`. This function only handles RBAC checks (project scope, workspace scope, permission resolution). It does NOT handle the "public repo unauthenticated read" case — that stays in the HTTP wrapper.

- Existing `check_access()` becomes a thin wrapper: (a) handle public-unauthenticated read → `Ok(None)`, (b) authenticate via HTTP Basic Auth, (c) call `check_access_for_user()`.
- `resolve_project()` is **already `pub`** at line 194 — no change needed.

### New file: `src/git/ssh_server.rs` (~500 LOC)

**Architecture**: Background task spawned in `main.rs`, like pipeline executor.

```rust
// In spawn_background_tasks():
if state.config.ssh_listen.is_some() {
    tokio::spawn(git::ssh_server::run(state.clone(), shutdown_tx.subscribe()));
}
```

**Function signature**: `pub async fn run(state: AppState, mut shutdown: tokio::sync::watch::Receiver<()>)` — follows the same shutdown pattern as `pipeline::executor::run()`.

**SSH server flow**:
1. Load or auto-generate ED25519 host key from `ssh_host_key_path`
   - **Security**: set file permissions to `0600` after generation; warn if existing key is world-readable (`mode & 0o077 != 0`)
2. Bind TCP listener on `ssh_listen` address
   - Use `tokio::select!` between `listener.accept()` and `shutdown.changed()` for graceful shutdown
3. Accept connections → `russh::server::run_stream()` with `GitSshHandler`
   - Configure `russh::server::Config` with `auth_rejection_time: Duration::from_secs(1)` and `max_auth_retries: 3`
4. `auth_publickey(user, key)`:
   - Compute fingerprint of presented key
   - Query `user_ssh_keys` by fingerprint, **JOIN users WHERE is_active = true** (critical — deactivated users must be rejected)
   - If found → `Auth::Accept`, store `GitUser` in handler state
   - Update `last_used_at` (fire-and-forget)
   - If not found → `Auth::Reject` (no explicit timing delay needed — russh's `auth_rejection_time` handles this)
   - **Like GitHub**: SSH username is ignored; user identified purely by key fingerprint
   - **No caching on the fingerprint→user lookup** — ensures deactivated users are rejected immediately
5. `exec_request(channel, command)` → delegates to `parse_ssh_command()` + `run_git_over_ssh()`:
   - **Command parsing** (extract to `parse_ssh_command()`):
     - Accept ONLY `git-upload-pack` or `git-receive-pack` as the command — reject everything else
     - Strip surrounding single or double quotes from the path argument
     - Strip leading `/` from path (git sends `/owner/repo.git` with SSH URLs)
     - Validate path contains exactly one `/` separating owner and repo
     - Reject `..`, null bytes, newlines, semicolons, pipes, backticks, `$`, spaces in path
     - Parse to `(owner, repo_name, is_read)` tuple
   - Resolve project via `smart_http::resolve_project()` — **never pass the raw SSH path to the filesystem**
   - Check access via `smart_http::check_access_for_user()`
   - **Git execution** (extract to `run_git_over_ssh()`):
     - Spawn `git upload-pack <repo_path>` (stateful, no `--stateless-rpc` for SSH)
     - Pipe SSH channel ↔ git subprocess stdin/stdout
6. On process exit (extract to `handle_post_push()`):
   - If `git-receive-pack`: run post-receive hooks (same as HTTP path — `hooks::post_receive()`)
   - Write audit log (`git.push`)
   - Send exit status, close channel
7. Graceful shutdown: stop accepting, wait for in-flight sessions (30s timeout)

**Helper extraction to avoid >100-line functions** (clippy `too_many_lines`):
- `parse_ssh_command(command: &str) -> Result<(String, String, bool), SshError>` — pure parsing + validation
- `run_git_over_ssh(channel, repo_path, service) -> Result<ExitStatus>` — subprocess I/O
- `handle_post_push(state, git_user, project, params) -> Result<()>` — hooks + audit

**Session state struct** (avoids `too_many_arguments`):
```rust
struct SshSession {
    state: AppState,
    git_user: Option<GitUser>,
}
```

### Modified files

| File | Change |
|------|--------|
| `src/git/mod.rs` | Add `pub mod ssh_server;` |
| `src/git/smart_http.rs` | Extract `check_access_for_user()` as pub fn (`resolve_project()` is already pub) |
| `src/config.rs` | Add `ssh_listen`, `ssh_host_key_path` |
| `src/main.rs` | Conditionally spawn SSH server in `spawn_background_tasks()` |
| `Cargo.toml` | Add `russh`, `russh-keys` |
| `deny.toml` | Add wrapper exceptions if needed |

### Tests

#### Tests to write FIRST (before implementation)

**Unit tests — `src/git/ssh_server.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_parse_exec_command_upload_pack` | `git-upload-pack 'owner/repo.git'` parsed correctly | Unit |
| `test_parse_exec_command_receive_pack` | `git-receive-pack 'owner/repo.git'` parsed correctly | Unit |
| `test_parse_exec_command_no_quotes` | `git-upload-pack owner/repo.git` (no quotes) parsed | Unit |
| `test_parse_exec_command_double_quotes` | `git-upload-pack "owner/repo.git"` parsed | Unit |
| `test_parse_exec_command_leading_slash` | `git-upload-pack '/owner/repo.git'` — slash stripped | Unit |
| `test_parse_exec_command_invalid_service` | `git-archive 'owner/repo'` rejected | Unit |
| `test_parse_exec_command_empty` | Empty command rejected | Unit |
| `test_parse_exec_command_path_traversal` | `git-upload-pack '../etc/passwd'` rejected | Unit |
| `test_parse_exec_command_absolute_path` | `git-upload-pack '/etc/passwd'` rejected | Unit |
| `test_parse_exec_command_no_path` | `git-upload-pack` with no path rejected | Unit |
| `test_parse_exec_command_shell_injection` | `git-upload-pack 'foo;rm -rf /'` rejected | Unit |
| `test_parse_exec_command_null_byte` | Path with `\0` rejected | Unit |
| `test_parse_owner_repo_strip_git_suffix` | `owner/repo.git` → (owner, repo) | Unit |
| `test_parse_owner_repo_no_suffix` | `owner/repo` → (owner, repo) | Unit |
| `test_parse_owner_repo_nested_slash_rejected` | `owner/sub/repo` rejected | Unit |

**Unit tests — `src/config.rs` (extend existing)**

| Test | Validates | Layer |
|---|---|---|
| `test_default_ssh_listen_none` | `ssh_listen` defaults to `None` | Unit |
| `test_default_ssh_host_key_path` | `ssh_host_key_path` has default value | Unit |

**Integration tests — `tests/ssh_server_integration.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_check_access_for_user_public_read_ok` | Public project read with GitUser passes | Integration |
| `test_check_access_for_user_private_read_with_permission` | Private project read with ProjectRead succeeds | Integration |
| `test_check_access_for_user_private_read_no_permission` | Private read without permission → NotFound | Integration |
| `test_check_access_for_user_write_with_permission` | Write with ProjectWrite succeeds | Integration |
| `test_check_access_for_user_write_no_permission` | Write without permission → NotFound | Integration |
| `test_check_access_for_user_internal_read_any_user` | Internal project read by any user succeeds | Integration |
| `test_check_access_for_user_scope_project_mismatch` | Token scoped to project A cannot access project B | Integration |
| `test_check_access_for_user_scope_workspace_mismatch` | Token scoped to workspace X cannot access project outside | Integration |
| `test_ssh_key_fingerprint_lookup` | Query `user_ssh_keys` by fingerprint returns user_id | Integration |
| `test_ssh_key_inactive_user_rejected` | Fingerprint matches but `is_active=false` → no match | Integration |

**E2E tests — `tests/e2e_ssh.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_ssh_clone_with_ed25519_key` | Full SSH clone succeeds | E2E |
| `test_ssh_push_with_ed25519_key` | Full SSH push succeeds, post-receive hooks fire | E2E |
| `test_ssh_clone_private_repo_denied_no_key` | SSH clone without matching key fails | E2E |
| `test_ssh_push_no_write_perm_denied` | SSH push without ProjectWrite denied | E2E |
| `test_ssh_last_used_at_updated` | After SSH auth, `last_used_at` is updated | E2E |

**Total: 17 unit + 10 integration + 5 E2E = 32 tests**

#### Existing tests to UPDATE

| File | Change | Reason |
|---|---|---|
| `tests/helpers/mod.rs` | Add `ssh_listen: None, ssh_host_key_path: "/tmp/test-ssh-key".into()` to Config | New Config fields |
| `tests/e2e_helpers/mod.rs` | Same Config update | New Config fields |
| `src/config.rs::Config::test_default()` | Add new fields | Compile-time requirement |

**No existing HTTP git tests should break** — the `check_access()` refactoring preserves the existing wrapper behavior.

#### Branch coverage checklist

| Branch/Path | Test |
|---|---|
| Command parsing (upload-pack, receive-pack) | `test_parse_exec_command_upload_pack`, `_receive_pack` |
| Command parsing (no quotes, double quotes, leading slash) | `test_parse_exec_command_no_quotes`, `_double_quotes`, `_leading_slash` |
| Invalid service rejected | `test_parse_exec_command_invalid_service` |
| Path traversal / injection rejected | `test_parse_exec_*_path_traversal`, `_shell_injection`, `_null_byte` |
| Empty/missing path | `test_parse_exec_command_empty`, `_no_path` |
| `auth_publickey` found + active | `test_ssh_clone_with_ed25519_key` + `test_ssh_key_fingerprint_lookup` |
| `auth_publickey` found + inactive | `test_ssh_key_inactive_user_rejected` |
| `auth_publickey` not found | `test_ssh_clone_private_repo_denied_no_key` |
| `last_used_at` update | `test_ssh_last_used_at_updated` |
| `check_access_for_user` all branches | 8 integration tests |
| Host key auto-generation | E2E tests (generated on first start) |
| `ssh_listen = None` → no server | All existing tests (SSH never started) |

#### Tests NOT needed

| Excluded | Why |
|---|---|
| SSH protocol handshake tests | `russh` handles wire protocol |
| Concurrent session tests | `russh` handles concurrency internally |
| `check_access()` HTTP wrapper regression | Behavior unchanged; 30+ existing smart_http tests cover it |

#### E2E test setup notes

- Start SSH server on random port: `TcpListener::bind("127.0.0.1:0")` → configure `ssh_listen`
- Generate fixed ED25519 test key pair, register via API
- Use `GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -i <keyfile> -p <port>"` for git operations
- Git repos under `/tmp/platform-e2e/`

---

## PR 3: GPG Key Management (API + Database)

### Migration

**`migrations/20260226030001_user_gpg_keys.up.sql`**:
```sql
CREATE TABLE user_gpg_keys (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_id           TEXT NOT NULL,             -- last 16 hex chars of fingerprint
    fingerprint      TEXT NOT NULL UNIQUE,      -- full 40+ hex fingerprint
    public_key_armor TEXT NOT NULL,             -- ASCII-armored public key
    public_key_bytes BYTEA NOT NULL,            -- serialized key for pgp verification
    emails           TEXT[] NOT NULL DEFAULT '{}', -- UID emails extracted from key
    expires_at       TIMESTAMPTZ,              -- NULL = no expiry
    can_sign         BOOLEAN NOT NULL DEFAULT true,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_user_gpg_keys_user ON user_gpg_keys(user_id);
CREATE INDEX idx_user_gpg_keys_key_id ON user_gpg_keys(key_id);
CREATE INDEX idx_user_gpg_keys_emails ON user_gpg_keys USING GIN (emails);
```

**`migrations/20260226030001_user_gpg_keys.down.sql`**:
```sql
DROP TABLE IF EXISTS user_gpg_keys;
```

**Schema notes:**
- `TEXT[]` for `emails` works fine with sqlx (7 existing tables use `TEXT[]` columns — maps to `Vec<String>`).
- GIN index on `emails` is correct for `@>` / `&&` array containment queries. First GIN index in this project — use array operators in queries to benefit from the index.
- Keys are immutable after creation, so no `updated_at` needed.

### New dependency

`Cargo.toml`: `pgp = "~0.15"` — pure Rust (MIT/Apache-2.0). NOT `sequoia-openpgp` (LGPL, blocked by `deny.toml`). **Pin with tilde** (`~0.15`) to prevent semver-range pulling `0.16+` / `0.19` which may have different MSRV. `pgp 0.15.0` is edition 2021, MSRV 1.81 — compatible with our Rust toolchain.

Run `cargo deny check` immediately after adding to verify transitive deps pass license + ban checks.

### New files

**`src/git/gpg_keys.rs`** (~200 LOC) — parsing + validation:
- `parse_gpg_public_key(armor: &str) -> Result<ParsedGpgKey, GpgKeyError>` — decode armor, extract key_id, fingerprint, UID emails, expiry, signing capability
- **Run parsing in `tokio::task::spawn_blocking`** — the `pgp` crate does CPU-intensive work; avoid blocking the async runtime
- `verify_email_match(key_emails: &[String], user_email: &str) -> bool` — case-insensitive comparison
- Error enum:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum GpgKeyError {
      #[error("invalid PGP public key armor")]
      InvalidArmor,
      #[error("key has no signing capability")]
      NoSignCapability,
      #[error("key has expired")]
      Expired,
      #[error("failed to extract key metadata")]
      MetadataError,
  }
  impl From<GpgKeyError> for ApiError {
      fn from(err: GpgKeyError) -> Self { ApiError::BadRequest(err.to_string()) }
  }
  ```

**`src/api/gpg_keys.rs`** (~200 LOC) — CRUD handlers:

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/users/me/gpg-keys` | AuthUser | List current user's GPG keys |
| POST | `/api/users/me/gpg-keys` | AuthUser | Add GPG public key |
| GET | `/api/users/me/gpg-keys/{id}` | AuthUser | Get key with full armor |
| DELETE | `/api/users/me/gpg-keys/{id}` | AuthUser | Delete GPG key |
| GET | `/api/admin/users/{user_id}/gpg-keys` | AdminUsers (scoped) | List any user's keys |

**Note:** Uses `/api/users/me/` prefix for consistency with existing `user_keys.rs`.

- Validation: public_key 100-100,000 chars, parse via `gpg_keys::parse_gpg_public_key()`
- **At least one UID email must match user's email** (enforced in handler at upload time)
- Max 50 GPG keys per user
- Fingerprint uniqueness by DB constraint
- **Rate limiting**: `check_rate(&state.valkey, "gpg_key_add", &auth.user_id.to_string(), 20, 300)` on POST
- Delete handler WHERE clause must include `AND user_id = auth.user_id`
- Admin endpoint must use `has_permission_scoped` — reuse `require_admin` helper
- Audit: `gpg_key.add`, `gpg_key.delete`
- All response types derive `Serialize, TS` with `#[ts(export)]`

### Modified files

| File | Change |
|------|--------|
| `src/git/mod.rs` | Add `pub mod gpg_keys;` |
| `src/api/mod.rs` | Add `pub mod gpg_keys;` + `.merge(gpg_keys::router())` |
| `Cargo.toml` | Add `pgp` |

### Tests

#### Tests to write FIRST (before implementation)

**Unit tests — `src/git/gpg_keys.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_parse_gpg_key_valid_rsa` | Valid RSA GPG key parses, extracts key_id/fingerprint/emails | Unit |
| `test_parse_gpg_key_valid_ed25519` | Valid ed25519 GPG key parses | Unit |
| `test_parse_gpg_key_extracts_multiple_uids` | Multiple UID emails extracted | Unit |
| `test_parse_gpg_key_extracts_key_id` | Key ID is last 16 hex chars of fingerprint | Unit |
| `test_parse_gpg_key_extracts_expiry` | Key with expiry returns correct `expires_at` | Unit |
| `test_parse_gpg_key_no_expiry` | Key without expiry returns `None` | Unit |
| `test_parse_gpg_key_can_sign` | Signing-capable key → `can_sign = true` | Unit |
| `test_parse_gpg_key_invalid_armor` | Garbage armor → error | Unit |
| `test_parse_gpg_key_empty_input` | Empty input → error | Unit |
| `test_parse_gpg_key_truncated_armor` | Truncated armor → error | Unit |
| `test_verify_email_match_exact` | Exact email match returns true | Unit |
| `test_verify_email_match_case_insensitive` | Case-insensitive match | Unit |
| `test_verify_email_match_no_match` | No match returns false | Unit |
| `test_verify_email_match_multiple_uids_one_match` | Multiple UIDs, one matches | Unit |
| `test_verify_email_match_empty_uids` | Empty UID list returns false | Unit |
| `test_fingerprint_format_hex` | Fingerprint is uppercase hex, 40+ chars | Unit |

**Integration tests — `tests/gpg_keys_integration.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_add_gpg_key_success` | POST returns 201 with correct fields | Integration |
| `test_list_gpg_keys_empty` | GET returns empty array | Integration |
| `test_list_gpg_keys_with_data` | After add, list returns key | Integration |
| `test_get_gpg_key_by_id` | GET by id returns full details + armor | Integration |
| `test_get_gpg_key_not_found` | GET nonexistent returns 404 | Integration |
| `test_get_gpg_key_other_users_key_not_found` | GET another user's key returns 404 | Integration |
| `test_delete_gpg_key_success` | DELETE returns 200, key gone | Integration |
| `test_delete_gpg_key_not_found` | DELETE nonexistent returns 404 | Integration |
| `test_delete_other_users_key_returns_not_found` | DELETE another user's key returns 404 | Integration |
| `test_add_duplicate_fingerprint_returns_conflict` | Same key twice returns 409 | Integration |
| `test_add_gpg_key_invalid_armor_returns_400` | Garbage returns 400 | Integration |
| `test_add_gpg_key_no_email_match_returns_400` | Key UIDs don't match user email → 400 | Integration |
| `test_add_gpg_key_public_key_too_short_returns_400` | <100 char key returns 400 | Integration |
| `test_add_gpg_key_public_key_too_long_returns_400` | >100000 char key returns 400 | Integration |
| `test_add_gpg_key_max_50_limit` | 51st key rejected | Integration |
| `test_list_gpg_keys_unauthenticated` | GET without token returns 401 | Integration |
| `test_add_gpg_key_unauthenticated` | POST without token returns 401 | Integration |
| `test_delete_gpg_key_unauthenticated` | DELETE without token returns 401 | Integration |
| `test_admin_list_user_gpg_keys` | Admin endpoint returns user's keys | Integration |
| `test_admin_list_gpg_keys_non_admin_denied` | Non-admin denied | Integration |
| `test_add_gpg_key_creates_audit_log` | Audit log has `gpg_key.add` | Integration |
| `test_delete_gpg_key_creates_audit_log` | Audit log has `gpg_key.delete` | Integration |
| `test_list_gpg_keys_only_own_keys` | Each user sees only their keys | Integration |

**Total: 16 unit + 23 integration = 39 tests**

#### Existing tests to UPDATE

None — PR 3 changes are additive.

#### Branch coverage checklist

| Branch/Path | Test |
|---|---|
| Parse valid RSA/ed25519 GPG keys | `test_parse_gpg_key_valid_rsa`, `_ed25519` |
| Multiple UIDs extraction | `test_parse_gpg_key_extracts_multiple_uids` |
| Expiry extraction / no expiry | `test_parse_gpg_key_extracts_expiry`, `_no_expiry` |
| Invalid/empty/truncated armor | `test_parse_gpg_key_invalid_armor`, `_empty_input`, `_truncated_armor` |
| Email match (exact, case-insensitive, no match, multiple, empty) | 5 `test_verify_email_match_*` tests |
| `GpgKeyError → BadRequest` | `test_add_gpg_key_invalid_armor_returns_400` |
| Email mismatch rejection | `test_add_gpg_key_no_email_match_returns_400` |
| Max 50 keys / fingerprint UNIQUE | `test_add_gpg_key_max_50_limit`, `_duplicate_*` |
| Delete own key / other user's key | `test_delete_gpg_key_success`, `_other_users_key_*` |
| Admin permission check | `test_admin_list_*` |
| Audit logging | `test_*_creates_audit_log` |

#### Tests NOT needed

| Excluded | Why |
|---|---|
| E2E tests for GPG key CRUD | CRUD is DB+API only |
| PGP crypto library internals | Tested by `pgp` crate |
| Key revocation | Not in scope |

#### Test key constants

Generate a real GPG test key pair using `gpg --batch --gen-key` and embed the ASCII-armored public key as a `const` in tests. The UID email must match the test user's email.

---

## PR 4: Commit Signature Verification

### New file: `src/git/signature.rs` (~250 LOC)

```rust
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub enum SignatureStatus { Verified, UnverifiedSigner, BadSignature, NoSignature }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SignatureInfo {
    pub status: SignatureStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_name: Option<String>,
}
```

**Verification flow**:
1. **Validate `sha` parameter** — must be 40 hex chars (SHA-1) or 64 hex chars (SHA-256). Reject invalid SHAs before passing to `git cat-file`.
2. `git cat-file commit <sha>` → extract raw commit object (use `Command::new("git").arg("cat-file").arg("commit").arg(&sha)` — never shell-interpolate)
3. Parse `gpgsig` header (multi-line, space-indented continuation)
4. If no gpgsig → `NoSignature`
5. Reconstruct signed data (commit object sans gpgsig header)
6. Parse detached signature → extract signing key ID
7. Look up `user_gpg_keys` by `key_id`
8. Verify signature against stored public key
9. If valid + key email matches commit author email → `Verified`
10. If valid + no email match → `UnverifiedSigner`
11. If invalid or no matching key → `BadSignature`

**Caching**: Cache results in Valkey with **project-scoped key**: `gpg:sig:{project_id}:{commit_sha}` (1 hour TTL). Include project_id to prevent cross-project cache pollution in forked repos. Invalidate relevant cache entries when a GPG key is deleted.

### Modify `src/git/browser.rs`

- Add `#[serde(skip_serializing_if = "Option::is_none")] pub signature: Option<SignatureInfo>` to `CommitInfo` struct — backwards-compatible (field absent when `None`)
- Add `verify_signatures: bool` query param to `CommitsQuery` (default: false, opt-in)
- When `verify_signatures=true`, call `verify_commit_signature()` for each returned commit (parallel via `futures::future::join_all`)
- Add new endpoint `GET /api/projects/{id}/commits/{sha}` — single commit detail, always verifies signature
- **SHA validation**: the `{sha}` path parameter must be validated as 7-64 hex characters before passing to git
- Run `just types` after this PR to regenerate TS types — `CommitInfo`, `SignatureInfo`, and `SignatureStatus` will be exported

### Modified files

| File | Change |
|------|--------|
| `src/git/mod.rs` | Add `pub mod signature;` |
| `src/git/browser.rs` | Add `signature` field, `verify_signatures` param, `commit_detail` endpoint |

### Tests

#### Tests to write FIRST (before implementation)

**Unit tests — `src/git/signature.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_parse_commit_object_with_gpgsig` | Raw commit with gpgsig: extracts signature + signed data | Unit |
| `test_parse_commit_object_without_gpgsig` | Raw commit without gpgsig → `None` | Unit |
| `test_parse_commit_object_multiline_gpgsig` | Space-indented continuation lines parsed | Unit |
| `test_reconstruct_signed_data` | Signed data = commit sans gpgsig header | Unit |
| `test_signature_status_display_verified` | `Verified` serializes correctly | Unit |
| `test_signature_status_display_no_signature` | `NoSignature` serializes correctly | Unit |
| `test_signature_status_display_bad_signature` | `BadSignature` serializes correctly | Unit |
| `test_signature_status_display_unverified_signer` | `UnverifiedSigner` serializes correctly | Unit |
| `test_extract_signing_key_id_from_signature` | Parse detached sig → extract key ID | Unit |
| `test_extract_signing_key_id_invalid_signature` | Invalid sig → None | Unit |
| `test_verify_signature_valid` | Valid sig + known key → Ok | Unit |
| `test_verify_signature_tampered_data` | Valid sig + tampered data → BadSignature | Unit |
| `test_verify_signature_wrong_key` | Valid sig + wrong key → BadSignature | Unit |
| `test_signature_info_serialization` | `SignatureInfo` serializes to expected JSON | Unit |
| `test_commit_info_with_signature_field` | `CommitInfo { signature: Some(...) }` serializes | Unit |

**Unit tests — `src/git/browser.rs` (extend existing)**

| Test | Validates | Layer |
|---|---|---|
| `test_commits_query_verify_signatures_default_false` | Default `verify_signatures = false` | Unit |
| `test_commits_query_verify_signatures_true` | `verify_signatures=true` parsed | Unit |

**Integration tests — `tests/gpg_signature_integration.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_commits_without_verify_flag_no_signature` | Commits without flag → no `signature` field | Integration |
| `test_commits_with_verify_flag_unsigned` | Unsigned commits → `NoSignature` status | Integration |
| `test_commit_detail_endpoint_unsigned` | `GET /commits/{sha}` on unsigned commit → `NoSignature` | Integration |
| `test_commit_detail_nonexistent_sha_returns_404` | Bad SHA → 404 | Integration |
| `test_commit_detail_unauthenticated_returns_401` | No auth on private project → 401 | Integration |
| `test_commits_verify_flag_respects_project_read` | User without ProjectRead denied | Integration |
| `test_signature_cache_hit` | Second call served from Valkey cache | Integration |

**E2E tests — `tests/e2e_gpg.rs`**

| Test | Validates | Layer |
|---|---|---|
| `test_gpg_signed_commit_verified` | Push GPG-signed commit → `Verified` status | E2E |
| `test_gpg_signed_commit_unverified_signer` | Key not uploaded → `BadSignature` | E2E |
| `test_unsigned_commit_no_signature` | Unsigned commit → `NoSignature` | E2E |

**Total: 17 unit + 7 integration + 3 E2E = 27 tests**

#### Existing tests to UPDATE

| File | Change | Reason |
|---|---|---|
| `src/git/browser.rs::tests::test_parse_log_normal` | May need `signature: None` in expected struct | `CommitInfo` gains new field |

**No other breakage expected** — `signature` is `Option` with `skip_serializing_if`, so existing JSON assertions pass.

#### Branch coverage checklist

| Branch/Path | Test |
|---|---|
| Commit has gpgsig / no gpgsig | `test_parse_commit_object_with_gpgsig`, `_without_gpgsig` |
| Multiline continuation | `test_parse_commit_object_multiline_gpgsig` |
| Signed data reconstruction | `test_reconstruct_signed_data` |
| Key ID extraction (valid / invalid) | `test_extract_signing_key_id_*` |
| Signature verification (valid / tampered / wrong key) | `test_verify_signature_*` |
| No gpgsig → NoSignature | `test_commits_with_verify_flag_unsigned` + E2E |
| Key found + email match → Verified | `test_gpg_signed_commit_verified` |
| Key not found → BadSignature | `test_gpg_signed_commit_unverified_signer` |
| Valkey cache miss → compute + store | `test_signature_cache_hit` (first call) |
| Valkey cache hit → return cached | `test_signature_cache_hit` (second call) |
| `verify_signatures=false` → no signature field | `test_commits_without_verify_flag_*` |
| `commit_detail` (success / 404 / 401) | 3 integration tests |

#### Tests NOT needed

| Excluded | Why |
|---|---|
| Full PGP key generation in unit tests | Use pre-generated test keys as constants |
| Concurrent verification stress | `join_all` tested by multi-commit E2E |
| Valkey TTL expiry | TTL is a Valkey feature |

#### E2E test setup notes

- Generate a test GPG key: `gpg --batch --gen-key` with fixed passphrase
- Upload GPG public key via API before pushing signed commits
- Use `git -c gpg.program=gpg -c commit.gpgsign=true commit -m "signed"` with the test key
- Verify via `GET /api/projects/{id}/commits?ref=main&verify_signatures=true`

---

## New Environment Variables

| Env Var | Default | Purpose |
|---------|---------|---------|
| `PLATFORM_SSH_LISTEN` | (empty = disabled) | SSH server listen address, e.g., `0.0.0.0:2222` |
| `PLATFORM_SSH_HOST_KEY_PATH` | `/data/ssh_host_ed25519_key` | SSH host key file (auto-generated if missing) |

---

## File Summary

### New files (12)
| File | Est. LOC |
|------|----------|
| `migrations/20260226010001_user_ssh_keys.{up,down}.sql` | 15 |
| `migrations/20260226030001_user_gpg_keys.{up,down}.sql` | 20 |
| `src/git/ssh_keys.rs` | 150 |
| `src/git/ssh_server.rs` | 500 |
| `src/git/gpg_keys.rs` | 200 |
| `src/git/signature.rs` | 250 |
| `src/api/ssh_keys.rs` | 200 |
| `src/api/gpg_keys.rs` | 200 |

### Modified files (8)
| File | Change |
|------|--------|
| `Cargo.toml` | Add `ssh-key 0.6`, `russh 0.49`, `russh-keys 0.49`, `pgp ~0.15` |
| `deny.toml` | Wrapper exceptions for russh if needed |
| `src/config.rs` | Add `ssh_listen`, `ssh_host_key_path` + update `test_default()` |
| `src/main.rs` | Conditionally spawn SSH server in `spawn_background_tasks()` |
| `src/git/mod.rs` | 4 new `pub mod` declarations |
| `src/git/smart_http.rs` | Extract `check_access_for_user()` as pub fn |
| `src/git/browser.rs` | Add signature field + `verify_signatures` param + `commit_detail` endpoint |
| `src/api/mod.rs` | 2 new `pub mod` + `.merge()` calls |

### New test files (6)
| File | Tests |
|------|-------|
| `tests/ssh_keys_integration.rs` | 23 |
| `tests/ssh_server_integration.rs` | 10 |
| `tests/gpg_keys_integration.rs` | 23 |
| `tests/gpg_signature_integration.rs` | 7 |
| `tests/e2e_ssh.rs` | 5 |
| `tests/e2e_gpg.rs` | 3 |

**Total: ~1,535 source LOC + ~2,100 test LOC (136 tests: 65 unit + 63 integration + 8 E2E)**

---

## PR Order & Dependencies

```
PR 1: SSH Key Management ──→ PR 2: SSH Protocol Server
PR 3: GPG Key Management ──→ PR 4: Commit Signature Verification
```

PR 1 and PR 3 are independent. I'll implement them in order: PR1 → PR2 → PR3 → PR4.

---

## Verification

After each PR:
- `just ci` (fmt + lint + deny + test-unit + build)
- `just test-integration` (after PR 1, 3 for API tests)
- `just test-e2e` (after PR 2, 4 for SSH/GPG E2E)

Final verification:
- SSH: `git clone ssh://git@localhost:2222/admin/test.git` with an ED25519 key
- GPG: push a signed commit, browse commits with `?verify_signatures=true`, confirm "verified" status

---

## Test Plan Summary

### Coverage target: 100% of touched lines

Every new or modified line of code must be covered by at least one test (unit, integration, or E2E). The test strategy above maps each code path to a specific test. `review` and `finalize` will verify this with `just cov-unit` / `just cov-total`.

### New test counts by PR

| PR | Unit | Integration | E2E | Total |
|---|---|---|---|---|
| PR 1: SSH Key Management | 15 | 23 | 0 | 38 |
| PR 2: SSH Protocol Server | 17 | 10 | 5 | 32 |
| PR 3: GPG Key Management | 16 | 23 | 0 | 39 |
| PR 4: Commit Signature Verification | 17 | 7 | 3 | 27 |
| **Total** | **65** | **63** | **8** | **136** |

### Coverage goals by module

| Module | Current tests | After plan |
|---|---|---|
| `src/git/ssh_keys.rs` | 0 | +15 unit |
| `src/git/ssh_server.rs` | 0 | +15 unit |
| `src/git/gpg_keys.rs` | 0 | +16 unit |
| `src/git/signature.rs` | 0 | +15 unit |
| `src/git/smart_http.rs` | 6 unit | +10 integration (for `check_access_for_user`) |
| `src/git/browser.rs` | 9 unit | +2 unit, +7 integration |
| `src/api/ssh_keys.rs` | 0 | +23 integration |
| `src/api/gpg_keys.rs` | 0 | +23 integration |
| E2E (SSH) | 0 | +5 E2E |
| E2E (GPG) | 0 | +3 E2E |

### Key implementation notes for test authors

1. **SSH key test constants**: Embed real ed25519/RSA/ECDSA key pairs as `const` strings. Generate with `ssh-keygen -t ed25519 -f /dev/stdout -N ""`.
2. **GPG key test constants**: Generate with `gpg --batch --gen-key`, embed ASCII-armored public key. UID email must match test user's email.
3. **Dynamic queries in tests**: Use `sqlx::query()` (not `sqlx::query!()`).
4. **Test state**: Use `helpers::test_state(pool).await` — never `admin_login()`.
5. **E2E SSH**: Start SSH server on random port, use `GIT_SSH_COMMAND` with `-o StrictHostKeyChecking=no`.
6. **Valkey cache tests**: Verify cache key exists with `fred::interfaces::KeysInterface::exists()`.

---

## Plan Review Findings

**Date:** 2026-02-26
**Status:** APPROVED WITH CONCERNS

### Codebase Reality Check

Issues corrected in-place above:
1. **API path prefix**: Plan used `/api/user/` — existing convention is `/api/users/me/`. Fixed to match `user_keys.rs` pattern.
2. **`ssh-key` version**: Plan specified `0.7` — doesn't exist as stable. `russh 0.49` uses `ssh-key 0.6`. Fixed to `0.6`.
3. **`pgp` version pinning**: `pgp = "0.15"` semver range would pull `0.19.0`. Fixed to `~0.15`.
4. **`resolve_project()` already public**: Plan said "ensure it's public" — it already is at `smart_http.rs:194`. Removed from modified list.
5. **`check_access_for_user()` return type**: Plan had ambiguous signature. Clarified to `Result<(), ApiError>` (caller already has `GitUser`).
6. **Redundant schema constraints**: Triple uniqueness on `user_ssh_keys.fingerprint`. Reduced to single `UNIQUE` column constraint.
7. **Missing DOWN migrations**: Plan didn't show them. Added.
8. **Missing `Config::test_default()` update**: Would break every test at compile time. Added.
9. **Missing `#[ts(export)]`** on `SignatureInfo`/`SignatureStatus`. Added.
10. **Missing `skip_serializing_if`** on `CommitInfo.signature`. Added for backwards compatibility.
11. **Missing SSH server `shutdown_rx`**: Added `tokio::sync::watch::Receiver<()>` parameter.
12. **Missing SSH command parsing validation**: Added explicit security spec for rejecting injection.
13. **Missing helper extraction**: SSH server would have >100-line functions. Added extraction plan.

### Remaining Concerns

1. **`rand` version mismatch**: `russh 0.49` uses `rand 0.8`/`rand_core 0.6`; platform uses `rand 0.10`/`rand_core 0.9`. Both compile but cannot share RNG types. **Action**: Never pass RNG instances between russh and platform code. Document the isolation boundary.

2. **GPG email match drift**: If a user changes their email after uploading a GPG key, the key's UID emails no longer match. At verification time (PR 4), the code checks the key email against the *commit* author email, not the user's current platform email. **Action**: Document this as a known limitation. Consider adding a re-validation step when users change their email (future work).

3. **SSH server connection rate limiting**: No per-IP connection throttling. `russh::Config::auth_rejection_time` provides some protection, but a dedicated DDoS attacker could open many connections. **Action**: Start with `russh` built-in limits (`auth_rejection_time`, `max_auth_retries`). Add a global `Semaphore(1000)` for max concurrent connections if needed post-launch.

4. **GPG parsing DoS**: Large PGP armor (up to 100K chars) could be CPU-intensive to parse. **Action**: `spawn_blocking` + timeout (5s) is specified in the plan. If profiling shows issues, lower the max to 50K.

### Simplification Opportunities

- **PR 1 schema**: Removed `public_key BYTEA` column — `public_key_openssh TEXT` is sufficient as the single source of truth. Raw bytes can be re-derived by `ssh-key` crate if needed.
- **Removed redundant index**: `idx_user_ssh_keys_fingerprint` was redundant with the `UNIQUE` constraint on `fingerprint`.

### Security Notes

- **SSH command injection** is the highest-risk surface. The plan now specifies exact validation rules (reject `..`, null bytes, shell metacharacters, require exactly `owner/repo` format). Never pass raw SSH-provided paths to the filesystem.
- **SSH host key permissions**: Set `0600` on generated key files. Warn on startup if permissions are too open.
- **Admin endpoints**: Must use `has_permission_scoped` (not `has_permission`) to respect API token scope restrictions.
- **Commit SHA validation**: New `commits/{sha}` endpoint must validate SHA as hex before passing to `git cat-file`.
- **GPG signature cache**: Keyed by `{project_id}:{commit_sha}` to prevent cross-project pollution.
- **Audit logging**: SSH auth failures should be logged at WARN level with source IP and fingerprint prefix (first 8 chars).
