# Implementation Plan: Security Hardening — Backlog Items

**Source:** `plans/security-audit-2026-03-24.md` — 12 BACKLOG items that need design decisions or upstream fixes
**Prerequisites:** All 46 actionable findings already implemented

## Implementation Progress

| Step | Finding | Status | Notes |
|---|---|---|---|
| 1 | S44 | ✅ Done | Version-prefixed encrypt (`0x01`), decrypt with previous key fallback, 6 new tests |
| 2 | S54 | ✅ Done | Immutable `v*` tags — block overwrite, allow idempotent same-digest push |
| 3 | S55 | ⬜ Deferred | MinIO HTTPS — needs minio.yaml cert setup + opendal TLS config |
| 4 | S59 | ✅ Done | Proxy trust CIDR — `trust_proxy_cidrs` config + ConnectInfo IP check, 8 new tests |
| 5 | S68 | ✅ Done | russh 0.49 still uses affected `rsa` crate — documented in deny.toml |
| 6 | S71 | ✅ Done | `token_max_expiry_days` config (default 365), enforced in create_api_token |
| 7 | S77 | ⬜ Deferred | DNS rebinding — requires making `check_ssrf_url` async (many call sites) |
| 8 | S78 | ⬜ Deferred | Image allowlist — needs migration + project API + definition validation |
| 9 | S84 | ✅ Done | Already covered — ensure_namespace applies NP to test namespaces |
| 10 | S85 | ✅ Done | Gateway allowedRoutes restricted to `platform.io/managed-by` label |
| 11 | S93 | ✅ Done | Tag names validated — reject `..`, null, `;`, `|`, `$`, backtick |
| 12 | S94 | ✅ Done | Hourly retention purge task — configurable `observe_retention_days` (default 30) |

---

## Step 1: S44 — Master key rotation (multi-key support)

**Problem:** No mechanism to rotate `PLATFORM_MASTER_KEY`. Compromise is permanent.

**Design:** Multi-key support with key_version in the encrypted blob. Zero downtime — both keys work during transition.

### 1a. Add key_version prefix to encrypted blobs

**File:** `src/secrets/engine.rs`

Current format: `nonce(12) || ciphertext || tag`
New format: `version(1) || nonce(12) || ciphertext || tag`

- Version `0x01` = current master key
- On encrypt: always use the current key, prepend `0x01`
- On decrypt: read first byte, select key accordingly. If `0x00` or no prefix (legacy), use current key (backwards compatible with existing data)

### 1b. Add config for old key

**File:** `src/config.rs`

```rust
pub master_key: Option<String>,
pub master_key_previous: Option<String>,  // PLATFORM_MASTER_KEY_PREVIOUS
```

### 1c. Modify encrypt/decrypt

**File:** `src/secrets/engine.rs`

```rust
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let mut nonce_bytes = [0u8; 12];
    rand::fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let ciphertext = cipher.encrypt(nonce, plaintext).expect("encryption failed");
    // version(1) + nonce(12) + ciphertext+tag
    let mut out = Vec::with_capacity(1 + 12 + ciphertext.len());
    out.push(0x01); // key version 1 = current key
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn decrypt(keys: &KeyRing, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
    if data.is_empty() {
        anyhow::bail!("empty ciphertext");
    }
    let (key, payload) = match data[0] {
        0x01 => (&keys.current, &data[1..]),
        0x02 => keys.previous.as_ref()
            .ok_or_else(|| anyhow::anyhow!("key version 2 but no previous key configured"))
            .map(|k| (k, &data[1..]))?,
        _ => (&keys.current, data), // legacy: no version prefix
    };
    // nonce(12) + ciphertext+tag
    let nonce = Nonce::from_slice(&payload[..12]);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher.decrypt(nonce, &payload[12..])
        .map_err(|_| anyhow::anyhow!("decryption failed"))
}
```

### 1d. Add KeyRing struct

```rust
pub struct KeyRing {
    pub current: [u8; 32],
    pub previous: Option<[u8; 32]>,
}
```

### 1e. Background re-encryption migration

**File:** `src/secrets/engine.rs` — new function:

```rust
pub async fn rotate_secrets(pool: &PgPool, keys: &KeyRing) -> anyhow::Result<u64> {
    // SELECT all secrets with legacy format (no version prefix or version != 0x01)
    // For each: decrypt with old/legacy key, re-encrypt with current key, UPDATE
    // Return count of rotated secrets
}
```

Call from a CLI command or admin API endpoint (not automatic — operator-initiated).

### 1f. Migration

New migration: `ALTER TABLE secrets ADD COLUMN key_version SMALLINT NOT NULL DEFAULT 1;`
(Optional — the version is already in the blob prefix. DB column is for querying which secrets need rotation.)

### 1g. Tests

```
test_encrypt_decrypt_with_version_prefix
test_decrypt_legacy_format_without_prefix
test_decrypt_with_previous_key
test_rotate_secrets_re_encrypts_all
test_encrypt_always_uses_current_key
```

---

## Step 2: S54 — Immutable OCI tag policy (global)

**Problem:** OCI registry tags are mutable. Pushing to an existing tag (e.g., `v1.0.0`) silently overwrites.

**Design:** Global policy — all tags matching `v*` (semver-like) are immutable once pushed.

### 2a. Check for existing tag before manifest PUT

**File:** `src/registry/manifests.rs` — in the manifest push handler, before the `INSERT ... ON CONFLICT DO UPDATE`:

```rust
// S54: Immutable tag policy — tags matching v* are immutable once set
if let Some(ref tag) = tag_name {
    if tag.starts_with('v') {
        let existing = sqlx::query_scalar!(
            "SELECT manifest_digest FROM registry_tags WHERE repository_id = $1 AND tag = $2",
            repository_id, tag,
        ).fetch_optional(&state.pool).await?;

        if let Some(existing_digest) = existing {
            if existing_digest != manifest_digest {
                return Err(RegistryError::TagExists(format!(
                    "tag '{tag}' is immutable (already points to {existing_digest})"
                )));
            }
            // Same digest = idempotent push, allow
        }
    }
}
```

### 2b. Tests

```
test_immutable_tag_blocks_overwrite
test_immutable_tag_allows_same_digest (idempotent)
test_non_v_tag_allows_overwrite
test_latest_tag_allows_overwrite
```

---

## Step 3: S55 — MinIO HTTPS

**Problem:** MinIO connection uses HTTP. Object storage traffic unencrypted.

**Design:** Same pattern as S12 (Postgres TLS) — TLS in dev via self-signed cert init container, HTTPS in production via Bitnami chart.

### 3a. Dev/test cluster

**File:** `hack/test-manifests/minio.yaml` — add init container for self-signed cert, configure MinIO with `--certs-dir`.

**File:** `hack/test-in-cluster.sh` — change `MINIO_ENDPOINT` from `http://` to `https://` with `MINIO_INSECURE=true` (self-signed cert in dev).

### 3b. Application code

**File:** `src/main.rs` — the opendal S3 backend already supports HTTPS. Just change the default endpoint from `http://` to `https://` in production Helm values.

**File:** `helm/platform/values.yaml` — document that `MINIO_ENDPOINT` should use `https://` in production.

### 3c. Tests

Existing tests verify connectivity — if HTTPS works, they pass.

---

## Step 4: S59 — Proxy trust CIDR restriction

**Problem:** `PLATFORM_TRUST_PROXY` is all-or-nothing. When true, any source IP can spoof X-Forwarded-For.

**Design:** Keep boolean `PLATFORM_TRUST_PROXY` + add optional `PLATFORM_TRUST_PROXY_CIDR`.

### 4a. Add config

**File:** `src/config.rs`

```rust
pub trust_proxy_headers: bool,
pub trust_proxy_cidrs: Vec<ipnetwork::IpNetwork>,  // PLATFORM_TRUST_PROXY_CIDR
```

Parse from comma-separated env var: `PLATFORM_TRUST_PROXY_CIDR=10.42.0.0/16,172.16.0.0/12`

### 4b. Update IP extraction

**File:** `src/auth/middleware.rs` — in the IP extraction logic, when `trust_proxy_headers` is true AND `trust_proxy_cidrs` is non-empty, only parse X-Forwarded-For if the connecting IP matches one of the CIDRs.

```rust
fn should_trust_proxy(config: &Config, connecting_ip: Option<IpAddr>) -> bool {
    if !config.trust_proxy_headers {
        return false;
    }
    if config.trust_proxy_cidrs.is_empty() {
        return true; // backwards compatible — trust all when no CIDR specified
    }
    connecting_ip.map_or(false, |ip| {
        config.trust_proxy_cidrs.iter().any(|cidr| cidr.contains(ip))
    })
}
```

### 4c. Helm configmap

**File:** `helm/platform/templates/configmap.yaml` — add:
```yaml
{{- if .Values.platform.env.trustProxyCidr }}
PLATFORM_TRUST_PROXY_CIDR: {{ .Values.platform.env.trustProxyCidr | quote }}
{{- end }}
```

### 4d. Tests

```
test_trust_proxy_cidr_allows_matching_ip
test_trust_proxy_cidr_rejects_non_matching_ip
test_trust_proxy_no_cidr_trusts_all (backwards compat)
test_trust_proxy_false_ignores_cidr
```

---

## Step 5: S68 — Re-evaluate RSA advisory

**Problem:** RUSTSEC-2023-0071 (RSA Marvin Attack) suppressed in deny.toml. Affects russh's SSH server.

### 5a. Check upstream

```bash
cargo tree -p russh -i | grep rsa
cargo tree -p rsa
```

Check if russh 0.50+ uses a patched `rsa` crate. If yes, update `Cargo.toml` to require the fixed version.

### 5b. If not fixed upstream

Document in `deny.toml` with a re-evaluation date. Add a comment with the tracking issue URL.

### 5c. If fixed upstream

Remove the `RUSTSEC-2023-0071` entry from `deny.toml` `[advisories.ignore]` list. Run `just deny` to verify.

---

## Step 6: S71 — Configurable token expiry max

**Problem:** API tokens can be created with up to 365-day expiry. No way to enforce organizational policy.

**Design:** Configurable max via `PLATFORM_TOKEN_MAX_EXPIRY_DAYS` (default: 365).

### 6a. Add config

**File:** `src/config.rs`

```rust
pub token_max_expiry_days: u32,  // PLATFORM_TOKEN_MAX_EXPIRY_DAYS, default 365
```

### 6b. Enforce in token creation

**File:** `src/api/users.rs` — in `create_api_token` handler, validate:

```rust
let max_days = state.config.token_max_expiry_days;
if body.expires_in_days > max_days {
    return Err(ApiError::BadRequest(
        format!("token expiry cannot exceed {max_days} days")
    ));
}
```

### 6c. Helm values

**File:** `helm/platform/values.yaml`:
```yaml
tokenMaxExpiryDays: 365
```

### 6d. Tests

```
test_token_expiry_within_max_ok
test_token_expiry_exceeds_max_rejected
```

---

## Step 7: S77 — DNS rebinding mitigation

**Problem:** Webhook SSRF check validates hostname at URL-check time. DNS rebinding can resolve to a public IP initially, then to a private IP at request time.

**Design:** Resolve DNS at check time and verify the resolved IP, not just the hostname.

### 7a. Add IP resolution to check_ssrf_url

**File:** `src/validation.rs` — in `check_ssrf_url`, after parsing the URL and extracting the host, resolve it to IP addresses and check each:

```rust
// Resolve hostname to IPs and verify none are private
if let Ok(addrs) = tokio::net::lookup_host(format!("{}:0", host)).await {
    for addr in addrs {
        if is_private_ip(addr.ip()) {
            return Err(ApiError::BadRequest("URL resolves to private IP".into()));
        }
    }
}
```

**Note:** `check_ssrf_url` is currently sync. This change makes it async. All call sites already are in async contexts, so this is straightforward.

### 7b. Tests

```
test_ssrf_resolves_hostname_to_ip
test_ssrf_blocks_hostname_resolving_to_private_ip
```

---

## Step 8: S78 — Pipeline image allowlist

**Problem:** Any container image can be used in `.platform.yaml` pipeline steps. Typosquatting or malicious images.

**Design:** Per-project optional allowlist. When set, only images matching the patterns are allowed.

### 8a. Add project-level config

New migration: `ALTER TABLE projects ADD COLUMN allowed_pipeline_images TEXT[];`

When NULL (default), any image is allowed (backwards compatible). When set, images must match one of the patterns (glob-style: `docker.io/library/*`, `gcr.io/kaniko-project/*`).

### 8b. Validate in pipeline definition

**File:** `src/pipeline/definition.rs` — in `validate()`, when the project has `allowed_pipeline_images`, check each step image against the allowlist.

### 8c. API endpoint

**File:** `src/api/projects.rs` — add `allowed_pipeline_images` to the project update handler.

### 8d. Tests

```
test_pipeline_image_allowed_by_pattern
test_pipeline_image_rejected_by_allowlist
test_pipeline_image_any_when_no_allowlist
```

---

## Step 9: S84 — Test namespace NetworkPolicy

**Problem:** `deploy_test` step creates temporary namespace without NetworkPolicy.

### 9a. Apply NetworkPolicy to test namespaces

**File:** `src/pipeline/executor.rs` — in the deploy_test step where the test namespace is created, apply the same `platform-managed` NetworkPolicy that `ensure_namespace` applies.

### 9b. Tests

Existing deploy_test integration tests verify functionality.

---

## Step 10: S85 — Gateway allowedRoutes restriction

**Problem:** Gateway resource allows routes from ALL namespaces.

### 10a. Restrict to platform-managed namespaces

**File:** `hack/cluster-up.sh` — change the Gateway spec:

```yaml
allowedRoutes:
  namespaces:
    from: Selector
    selector:
      matchLabels:
        platform.io/managed-by: platform
```

This restricts HTTPRoute creation to namespaces with the platform-managed label.

### 10b. Tests

Existing E2E tests for progressive delivery verify HTTPRoutes still work.

---

## Step 11: S93 — Tag name validation in git hooks

**Problem:** Tag names from pushed data passed to `git rev-parse` without validation.

### 11a. Validate tag names

**File:** `src/git/hooks.rs` — in `extract_pushed_tags`, validate each tag name:

```rust
for tag in &tags {
    if tag.contains("..") || tag.contains('\0') || tag.contains(';') || tag.contains('|') {
        tracing::warn!(%tag, "rejected tag with dangerous characters");
        continue; // skip, don't crash
    }
}
```

### 11b. Tests

```
test_extract_tags_rejects_path_traversal
test_extract_tags_rejects_null_bytes
test_extract_tags_allows_normal_tags
```

---

## Step 12: S94 — Observability data retention

**Problem:** Observability tables (spans, log_entries, metric_samples) grow unbounded. No purging.

### 12a. Add retention config

**File:** `src/config.rs`

```rust
pub observe_retention_days: u32,  // PLATFORM_OBSERVE_RETENTION_DAYS, default 30
```

### 12b. Background purge task

**File:** `src/observe/mod.rs` — add a background task (alongside the existing flush tasks):

```rust
async fn retention_cleanup(pool: &PgPool, retention_days: u32) {
    let cutoff = Utc::now() - Duration::days(retention_days as i64);
    sqlx::query("DELETE FROM spans WHERE timestamp < $1").bind(cutoff).execute(pool).await;
    sqlx::query("DELETE FROM log_entries WHERE timestamp < $1").bind(cutoff).execute(pool).await;
    sqlx::query("DELETE FROM metric_samples WHERE timestamp < $1").bind(cutoff).execute(pool).await;
    tracing::info!(retention_days, "observability data purged");
}
```

Run hourly (same interval as alert evaluation).

### 12c. Helm values

```yaml
observeRetentionDays: 30
```

### 12d. Tests

```
test_retention_deletes_old_data
test_retention_preserves_recent_data
```

---

## Execution Order

```
┌─ Quick (1-2 files, no migration) ──────────────────┐
│  Step 5  (S68): Check russh upstream RSA fix        │
│  Step 9  (S84): Test namespace NetworkPolicy        │
│  Step 10 (S85): Gateway allowedRoutes restriction   │
│  Step 11 (S93): Tag name validation                 │
└─────────────────────────────────────────────────────┘

┌─ Small (config + 1-2 files) ────────────────────────┐
│  Step 6  (S71): Configurable token expiry max       │
│  Step 3  (S55): MinIO HTTPS                         │
└─────────────────────────────────────────────────────┘

┌─ Medium (new feature) ──────────────────────────────┐
│  Step 2  (S54): Immutable OCI tags                  │
│  Step 4  (S59): Proxy trust CIDR                    │
│  Step 7  (S77): DNS rebinding mitigation            │
│  Step 12 (S94): Observability retention              │
└─────────────────────────────────────────────────────┘

┌─ Large (crypto + migration) ────────────────────────┐
│  Step 1  (S44): Master key rotation                 │
│  Step 8  (S78): Pipeline image allowlist            │
└─────────────────────────────────────────────────────┘
```

## Test Summary

| Step | Finding | Unit Tests | Integration Tests |
|---|---|---|---|
| 1 | S44 | 5 (encrypt/decrypt/rotate) | — |
| 2 | S54 | — | 4 (immutable/idempotent/overwrite) |
| 3 | S55 | — | existing verify |
| 4 | S59 | 4 (CIDR match/reject/compat) | — |
| 5 | S68 | — | — |
| 6 | S71 | — | 2 (within/exceeds max) |
| 7 | S77 | 2 (resolve/block) | — |
| 8 | S78 | — | 3 (allow/reject/no-list) |
| 9 | S84 | — | existing verify |
| 10 | S85 | — | existing verify |
| 11 | S93 | 3 (traversal/null/normal) | — |
| 12 | S94 | — | 2 (delete old/preserve recent) |
| **Total** | | **14** | **~11** |
