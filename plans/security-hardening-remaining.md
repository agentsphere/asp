# Implementation Plan: Security Hardening — Remaining Unique Findings

**Source:** `plans/security-audit-2026-03-24.md`
**Previous plans:** `s1-s20` (11 implemented, 1 deferred), `s20-s35` (6 implemented, 2 deferred)
**Scope:** All remaining UNIQUE findings not covered by A/E audits, grouped by effort

---

## Status of all UNIQUE findings

| Finding | Severity | Status | Plan |
|---|---|---|---|
| S3 | CRITICAL | ✅ Done | s1-s20 (PSA baseline + ResourceQuota + LimitRange) |
| S5 | CRITICAL | ✅ Accepted | DD-2 (agent needs secrets CRUD) |
| S6 | CRITICAL | ⚠ Partial | s1-s20 (implementation notes in report; Helm split deferred) |
| S7 | CRITICAL | ✅ Done | s1-s20 (Actions SHA-pinned) |
| S9 | CRITICAL | ✅ Done | s1-s20 (curl\|bash replaced) |
| S10 | CRITICAL | ✅ Done | s1-s20 (CI permissions: {}) |
| S11 | HIGH | ✅ Done | s1-s20 (workspace permissions query + cache invalidation) |
| S12 | HIGH | ✅ Done | s1-s20 (Postgres TLS) |
| S13 | HIGH | ✅ Done | s1-s20 (Valkey auth) |
| S15 | HIGH | ✅ Done | s1-s20 (pipeline step securityContext) |
| S16 | HIGH | ✅ Done | s1-s20 (pipeline registry_tag_pattern) |
| S19 | HIGH | ✅ Done | s1-s20 (deployer pod spec validation) |
| S20 | HIGH | ✅ Done | s1-s20 (workspace owner demotion) |
| S21 | HIGH | ✅ Done | s20-s35 (observe admin for unscoped queries) |
| S23 | HIGH | ⬜ Deferred | s20-s35 → **this plan, Step 1** |
| S24 | HIGH | ⬜ Deferred | s20-s35 → **this plan, Step 1** |
| S25 | HIGH | ✅ Done | s20-s35 (trustProxy false + auto-derive) |
| S30 | HIGH | ✅ Done | s20-s35 (namespace delete guard) |
| S31 | HIGH | ⬜ Deferred | s20-s35 → **this plan, Step 2** |
| S33 | HIGH | ✅ Done | s20-s35 (host mount dev_mode gate) |
| S34 | HIGH | ✅ Done | s20-s35 (npm --ignore-scripts) |
| S35 | HIGH | ✅ Done | s20-s35 (cargo-audit in CI) |
| S36 | MEDIUM | ⬜ | **this plan, Step 3** |
| S39 | MEDIUM | ⬜ | **this plan, Step 4** |
| S43 | MEDIUM | ⬜ | **this plan, Step 5** |
| S44 | MEDIUM | ⬜ Backlog | Design-heavy — key versioning + re-encryption migration |
| S45 | MEDIUM | ⬜ | **this plan, Step 6** |
| S47 | MEDIUM | ⬜ | **this plan, Step 7** |
| S48 | MEDIUM | ⬜ | **this plan, Step 8** |
| S51 | MEDIUM | ⬜ | **this plan, Step 9** |
| S52 | MEDIUM | ⬜ | **this plan, Step 10** |
| S53 | MEDIUM | ⬜ | **this plan, Step 10** (same pattern as S52) |
| S54 | MEDIUM | ⬜ Backlog | Needs project-level immutable tag policy design |
| S55 | MEDIUM | ⬜ Backlog | MinIO HTTPS — infra-level, similar to S12 approach |
| S57 | MEDIUM | ⬜ | **this plan, Step 1** (bundled with S23/S24) |
| S58 | MEDIUM | ⬜ | **this plan, Step 11** |
| S59 | MEDIUM | ⬜ Backlog | Needs CIDR parsing + config change — design first |
| S61 | MEDIUM | ⬜ | **this plan, Step 12** |
| S63 | MEDIUM | ⬜ | **this plan, Step 13** |
| S64 | MEDIUM | ⬜ | **this plan, Step 14** |
| S65 | MEDIUM | ⬜ | **this plan, Step 15** |
| S66 | MEDIUM | ⬜ | **this plan, Step 16** |
| S68 | MEDIUM | ⬜ Backlog | Requires russh upstream fix — track only |
| S6 Helm | CRITICAL | ⬜ Deferred | **this plan, Step 17** |
| **LOW** | | | |
| S70 | LOW | ⬜ | **this plan, Step 18** |
| S71 | LOW | ⬜ Backlog | Policy decision — 365d may be intentional |
| S77 | LOW | ⬜ Backlog | DNS rebinding — needs custom resolver |
| S78 | LOW | ⬜ Backlog | Image allowlist — needs project-level config design |
| S81 | LOW | ⬜ | **this plan, Step 19** |
| S82 | LOW | ⬜ | **this plan, Step 9** (bundled with S51 HSTS) |
| S84 | LOW | ⬜ Backlog | Test namespace NP — low priority |
| S85 | LOW | ⬜ Backlog | Gateway allowedRoutes — low priority |
| S89 | LOW | ⬜ | **this plan, Step 20** |
| S93 | LOW | ⬜ Backlog | Tag name validation — low priority |
| S94 | LOW | ⬜ Backlog | Observability retention — design-heavy |

**Backlog items** (5 MEDIUM + 7 LOW) need design decisions or upstream fixes — not implementable as simple code changes. Track separately.

## Implementation Progress

| Step | Finding | Status | Notes |
|---|---|---|---|
| 1 | S23/24/57 | ⬜ Deferred | Default-deny NP — needs E2E validation with pipelines |
| 2 | S31 | ⬜ Deferred | Git token Secret volume — large PodSpecParams refactor |
| 3 | S36 | ✅ Already done | Linter/previous work already added `current_password` verification |
| 4 | S39 | ✅ Done | Global rate limit on `begin_login` (60 per 60s) |
| 5 | S43 | ✅ Done | `write_audit("secret.read")` on secret read |
| 6 | S45 | ✅ Done | `auth.check_workspace_scope(id)?` on all 7 workspace handlers |
| 7 | S47 | ✅ Done | Delegation revoke checks delegator_id or admin:users |
| 8 | S48 | ✅ Done | Random dev password + random master key (no more all-zeros) |
| 9 | S51+S82 | ✅ Done | HSTS + Permissions-Policy headers added to main.rs |
| 10 | S52+S53 | ✅ Done | Rate limit on git HTTP auth (20/300s) + registry Basic auth (20/300s) |
| 11 | S58 | ✅ Done | Data store NP restricted to platform pod selector |
| 12 | S61 | ✅ Done | Merge errors sanitized — generic message to client, stderr to logs |
| 13 | S63 | ✅ Done | SSRF re-validation in `dispatch_single()` |
| 14 | S64 | ✅ Done | NodePort opt-in (`nodePort.enabled: false` default) |
| 15 | S65 | ✅ Done | Agent token expiry reduced from 24h to 2h |
| 16 | S66 | ✅ Done | `npm audit fix` — 0 vulnerabilities remaining |
| 17 | S6 Helm | ⬜ Deferred | ClusterRole secrets split |
| 18 | S70 | ✅ Done | Argon2 params: 64 MiB, 3 iterations, Argon2id (was 19 MiB default) |
| 19 | S81 | ✅ Done | Implicit TLS on port 465, STARTTLS on others |
| 20 | S89 | ✅ Done | `.expect()` → `.ok_or(RegistryError::Internal(...))` |

---

## Step 1: S23 + S24 + S57 — Default-deny NetworkPolicy for all managed namespaces

**Problem:** NetworkPolicy only targets agent-session pods. Pipeline pods and deployed workloads are unrestricted. No NP in prod namespaces.

### 1a. Add default-deny to ensure_namespace()

**File:** `src/deployer/namespace.rs` — `ensure_namespace()` is called for ALL managed namespaces (dev, staging, prod, session). Add a default-deny NP after creating the namespace:

```rust
let default_deny = json!({
    "apiVersion": "networking.k8s.io/v1",
    "kind": "NetworkPolicy",
    "metadata": { "name": "default-deny", "namespace": ns_name },
    "spec": {
        "podSelector": {},
        "policyTypes": ["Ingress", "Egress"]
    }
});
```

Gate on a `platform_namespace` parameter (needed for allow-policy egress rules). Skip in dev_mode (same pattern as session NP and PSA).

### 1b. Add pipeline egress allow policy to dev namespaces

Pipeline pods need DNS + internet egress + platform API access. Add alongside the default-deny:

```rust
let pipeline_allow = json!({
    "apiVersion": "networking.k8s.io/v1",
    "kind": "NetworkPolicy",
    "metadata": { "name": "pipeline-egress", "namespace": ns_name },
    "spec": {
        "podSelector": { "matchExpressions": [{"key": "platform.io/pipeline", "operator": "Exists"}] },
        "policyTypes": ["Egress"],
        "egress": [
            { "ports": [{"port": 53, "protocol": "UDP"}, {"port": 53, "protocol": "TCP"}] },
            { "to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": platform_namespace}}}],
              "ports": [{"port": 8080, "protocol": "TCP"}] },
            { "to": [{"ipBlock": {"cidr": "0.0.0.0/0", "except": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10"]}}] }
        ]
    }
});
```

### 1c. Modify ensure_namespace() signature

Add `dev_mode: bool` and `platform_namespace: &str` params. Update all call sites.

### 1d. Tests

```
test_managed_namespace_has_default_deny
test_dev_namespace_has_pipeline_egress_policy
test_prod_namespace_has_default_deny
```

---

## Step 2: S31 — Git auth token via K8s Secret volume

**Problem:** `GIT_AUTH_TOKEN` as plain env var visible in pod spec.

### 2a. Create K8s Secret before pod creation

**File:** `src/pipeline/executor.rs` — in the pipeline execution flow (where `create_registry_secret` is called), create a git auth Secret:

```rust
let git_secret_name = format!("pl-git-{}", &pipeline_id.to_string()[..8]);
// Create Secret with token
// Mount as volume in init container
// Read token via `cat /git-auth/token` in GIT_ASKPASS script
```

### 2b. Replace env var with volume mount in PodSpecParams

Remove `git_auth_token: &'a str` from `PodSpecParams`. Add `git_secret_name: Option<&'a str>`. Update `build_pod_spec()` to mount the Secret volume and use file-based GIT_ASKPASS.

### 2c. Clean up Secret after pipeline completes

Extend `cleanup_registry_secret()` to also delete the git auth Secret.

### 2d. Update all test call sites

~20 test instances of `PodSpecParams` need updating (remove `git_auth_token`, add `git_secret_name`).

### 2e. Tests

```
test_pipeline_init_container_no_git_token_env
test_pipeline_init_container_has_git_auth_volume
test_pipeline_git_secret_cleaned_up_after_completion
```

---

## Step 3: S36 — Require current password for password change

**Problem:** `PATCH /api/users/{id}` accepts new password without requiring current password.

### 3a. Add current_password field

**File:** `src/api/users.rs` — add `current_password: Option<String>` to the update request struct. When the caller is changing their OWN password (`id == auth.user_id`), require and verify `current_password`. Admins changing OTHER users' passwords skip this.

### 3b. Tests

```
test_self_password_change_requires_current_password
test_self_password_change_wrong_current_fails
test_admin_can_change_other_user_password_without_current
```

---

## Step 4: S39 — Rate limit passkey begin_login

**Problem:** Unauthenticated `POST /api/auth/passkey/login/begin` has no rate limit. Flood generates Valkey challenge objects.

### 4a. Add rate limit

**File:** `src/api/passkeys.rs` — at the top of `begin_login`:

```rust
let ip = auth_ip_from_request(&req); // or extract from headers
check_rate(&state.valkey, "passkey_begin", &ip, 30, 60).await?;
```

### 4b. Tests

```
test_passkey_begin_login_rate_limited
```

---

## Step 5: S43 — Audit log secret reads

**Problem:** `GET /api/projects/{id}/secrets/{name}` returns plaintext secret without audit trail.

### 5a. Add audit entry on read

**File:** `src/api/secrets.rs` — in the read handler, add `write_audit()` with action `"secret.read"`.

### 5b. Tests

```
test_secret_read_creates_audit_entry
```

---

## Step 6: S45 — Enforce workspace scope on workspace endpoints

**Problem:** API token with `boundary_workspace_id=A` can access workspace B. `check_workspace_scope()` exists but is never called.

### 6a. Add scope checks

**File:** `src/api/workspaces.rs` — add `auth.check_workspace_scope(id)?` at the start of: `get_workspace`, `update_workspace`, `delete_workspace`, `list_members`, `add_member`, `remove_member`, `list_workspace_projects`.

For `list_workspaces`: filter results by `auth.boundary_workspace_id` if set.

### 6b. Tests

```
test_scoped_token_cannot_access_other_workspace
test_scoped_token_can_access_own_workspace
test_unscoped_token_can_access_any_workspace
```

---

## Step 7: S47 — Delegation revocation ownership check

**Problem:** Any user with `admin:delegate` can revoke delegations created by other admins.

### 7a. Add ownership check

**File:** `src/api/admin.rs:437-462` — before deleting a delegation, verify `auth.user_id == delegation.delegator_id` OR the caller has `admin:users` (full admin).

### 7b. Tests

```
test_delegator_can_revoke_own_delegation
test_non_delegator_cannot_revoke_others_delegation
test_admin_can_revoke_any_delegation
```

---

## Step 8: S48 — Dev mode random credentials

**Problem:** Dev mode uses predictable password "admin" and all-zeros master key.

### 8a. Generate random dev password

**File:** `src/store/bootstrap.rs` — when `dev_mode && admin_password.is_none()`, generate a random 16-char password and log it via `tracing::warn!` (same as prod setup token pattern).

### 8b. Generate random dev master key

**File:** `src/main.rs` — when `dev_mode && master_key.is_none()`, generate 32 random bytes (hex-encoded) instead of all-zeros. Log a warning but not the key itself.

### 8c. Tests

```
test_dev_mode_generates_random_admin_password (unit — bootstrap)
test_dev_mode_master_key_not_all_zeros (unit — config)
```

---

## Step 9: S51 + S82 — Add HSTS and Permissions-Policy headers

**Problem:** Missing `Strict-Transport-Security` and `Permissions-Policy` headers.

### 9a. Add headers in main.rs middleware

**File:** `src/main.rs` — alongside existing `X-Frame-Options`, `X-Content-Type-Options`, `Referrer-Policy`:

```rust
// HSTS — only when secure cookies are enabled (implies TLS)
if config.secure_cookies {
    .layer(SetResponseHeaderLayer::overriding(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    ))
}

// Permissions-Policy — disable unused browser features
.layer(SetResponseHeaderLayer::overriding(
    HeaderName::from_static("permissions-policy"),
    HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
))
```

### 9b. Tests

```
test_response_has_permissions_policy_header
test_response_has_hsts_when_secure_cookies (integration — needs config toggle)
```

---

## Step 10: S52 + S53 — Rate limit Git HTTP auth and registry auth

**Problem:** Unlimited password/token guessing on git clone/push and docker pull/push.

### 10a. Git HTTP auth rate limit

**File:** `src/git/smart_http.rs` — in `authenticate_basic()` or `check_access()`:

```rust
check_rate(&state.valkey, "git_auth", &username, 20, 300).await?;
```

### 10b. Registry auth rate limit

**File:** `src/registry/auth.rs` — in `RegistryUser` extractor:

```rust
check_rate(&state.valkey, "registry_auth", &username, 20, 300).await?;
```

### 10c. Tests

```
test_git_http_auth_rate_limited
test_registry_auth_rate_limited
```

---

## Step 11: S58 — Tighten data store NetworkPolicy

**Problem:** Data store NP allows all pods in the platform namespace.

### 11a. Restrict to platform pod selector

**File:** `helm/platform/templates/networkpolicy-data.yaml` — change ingress `from` to match only the platform pod's labels instead of the entire namespace:

```yaml
ingress:
  - from:
      - podSelector:
          matchLabels:
            app.kubernetes.io/name: {{ include "platform.name" . }}
```

### 11b. Tests

None — Helm template change. Verify with `helm template`.

---

## Step 12: S61 — Sanitize git merge error responses

**Problem:** Git stderr (file paths, version info) returned verbatim in API response.

### 12a. Return generic message

**File:** `src/api/merge_requests.rs:1055,1058,1061` — replace:

```rust
// Before:
Err(e) => return Err(ApiError::BadRequest(format!("merge failed: {e}"))),

// After:
Err(e) => {
    tracing::warn!(error = %e, "git merge failed");
    return Err(ApiError::BadRequest("merge failed".into()));
}
```

### 12b. Tests

```
test_merge_conflict_returns_generic_error (verify no stderr in response body)
```

---

## Step 13: S63 — Re-validate webhook URL before dispatch

**Problem:** `dispatch_single()` fetches webhook URL without SSRF re-check.

### 13a. Add re-validation

**File:** `src/api/webhooks.rs` (or `src/notify/webhook.rs`) — in `dispatch_single()`, before the HTTP request:

```rust
if let Err(e) = validation::check_ssrf_url(&url) {
    tracing::warn!(webhook_id = %id, "webhook URL failed SSRF re-validation, skipping");
    return;
}
```

### 13b. Tests

```
test_dispatch_skips_ssrf_violating_url
```

---

## Step 14: S64 — Make NodePort service opt-in

**Problem:** NodePort service auto-created when ingress is disabled, exposing platform on every node.

### 14a. Add nodePort.enabled toggle

**File:** `helm/platform/values.yaml`:

```yaml
nodePort:
  enabled: false  # default: ClusterIP only
```

**File:** `helm/platform/templates/service-nodeport.yaml` — wrap in conditional:

```yaml
{{- if .Values.nodePort.enabled }}
...
{{- end }}
```

### 14b. Tests

None — Helm template. Verify with `helm template`.

---

## Step 15: S65 — Reduce agent token expiry

**Problem:** Agent tokens valid 24 hours but sessions complete in minutes.

### 15a. Reduce to 2 hours

**File:** `src/agent/identity.rs:98` — change token expiry from `interval '24 hours'` to `interval '2 hours'`.

### 15b. Tests

```
test_agent_token_expires_within_2_hours
```

---

## Step 16: S66 — Update MCP npm dependencies

**Problem:** Known HIGH vulnerabilities in hono and express-rate-limit.

### 16a. Update dependencies

```bash
cd mcp && npm audit fix && npm audit
```

If `npm audit fix` doesn't resolve, add overrides in `mcp/package.json`:

```json
"overrides": {
  "hono": ">=4.12.7",
  "express-rate-limit": ">=8.2.2"
}
```

### 16b. Tests

```bash
cd mcp && npm test
```

---

## Step 17: S6 (deferred) — Helm ClusterRole secrets split

**Problem:** ClusterRole grants secrets CRUD cluster-wide.

### 17a. New ClusterRole template

**New file:** `helm/platform/templates/clusterrole-secrets.yaml` — ClusterRole with only secrets CRUD (template, not bound cluster-wide).

### 17b. Remove secrets from main ClusterRole

**File:** `helm/platform/templates/clusterrole.yaml` — remove `secrets` from the core API group resources list.

### 17c. Create per-namespace RoleBinding in namespace.rs

**File:** `src/deployer/namespace.rs` — in `ensure_namespace()`, create a RoleBinding binding the platform SA to the secrets ClusterRole in each managed namespace.

### 17d. Update test RBAC

**File:** `hack/test-manifests/rbac.yaml` — mirror the Helm change.

### 17e. Tests

```
test_managed_namespace_has_secrets_rolebinding
```

---

## Step 18: S70 — Increase Argon2 memory cost

**Problem:** Default params (19 MiB) below OWASP minimum (46 MiB).

### 18a. Configure explicit params

**File:** `src/auth/password.rs` — replace `Argon2::default()` with explicit params:

```rust
use argon2::{Algorithm, Argon2, Params, Version};
let argon2 = Argon2::new(
    Algorithm::Argon2id,
    Version::V0x13,
    Params::new(65536, 3, 1, None).unwrap(), // 64 MiB, 3 iterations, 1 parallel
);
```

Apply in both `hash_password()` and the bootstrap admin creation.

### 18b. Tests

Existing password tests should pass (argon2 auto-detects params on verify).

---

## Step 19: S81 — SMTP TLS enforcement

**Problem:** `starttls_relay()` vulnerable to STARTTLS stripping.

### 19a. Use required TLS

**File:** `src/notify/email.rs` — add config option for TLS mode. Default to STARTTLS required:

```rust
// Use port 465 implicit TLS when configured, otherwise STARTTLS with required TLS
let transport = if config.smtp_port == 465 {
    AsyncSmtpTransport::<Tokio1Executor>::relay(&host)?
} else {
    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?
        .tls(Tls::Required(TlsParameters::new(host.into())?))
};
```

### 19b. Tests

None — requires SMTP server. Manual verification.

---

## Step 20: S89 — Replace expect() with proper error

**Problem:** `expect()` panic on NULL `workspace_id` in registry mod.

### 20a. Replace with error

**File:** `src/registry/mod.rs:99` — replace `expect("workspace_id present...")` with `.ok_or(RegistryError::Internal(...))?`.

### 20b. Tests

```
test_registry_handles_null_workspace_id
```

---

## Execution Order

```
 ┌─ Quick wins (1-2 lines each, independent) ──────────┐
 │  Step 5  (S43): Secret read audit log                │
 │  Step 12 (S61): Sanitize merge error response        │
 │  Step 13 (S63): Webhook dispatch SSRF re-check       │
 │  Step 15 (S65): Agent token 2h expiry                │
 │  Step 20 (S89): expect → proper error                │
 └──────────────────────────────────────────────────────┘

 ┌─ Small (5-15 lines, independent) ────────────────────┐
 │  Step 3  (S36): Current password on self-change      │
 │  Step 4  (S39): Passkey begin_login rate limit       │
 │  Step 6  (S45): Workspace scope enforcement          │
 │  Step 7  (S47): Delegation revocation ownership      │
 │  Step 8  (S48): Random dev credentials               │
 │  Step 9  (S51+S82): HSTS + Permissions-Policy        │
 │  Step 10 (S52+S53): Git + Registry auth rate limit   │
 │  Step 18 (S70): Argon2 params                        │
 └──────────────────────────────────────────────────────┘

 ┌─ Helm / infra (no Rust code) ────────────────────────┐
 │  Step 11 (S58): Data store NP pod selector           │
 │  Step 14 (S64): NodePort opt-in                      │
 │  Step 16 (S66): MCP npm update                       │
 │  Step 19 (S81): SMTP TLS enforcement                 │
 └──────────────────────────────────────────────────────┘

 ┌─ Medium (namespace.rs / executor.rs refactors) ──────┐
 │  Step 1  (S23/24/57): Default-deny NetworkPolicy     │
 │  Step 2  (S31): Git token Secret volume              │
 │  Step 17 (S6): Helm ClusterRole secrets split        │
 └──────────────────────────────────────────────────────┘

 ┌─ Backlog (need design / upstream) ───────────────────┐
 │  S44: Master key rotation                            │
 │  S54: Immutable tag policy                           │
 │  S55: MinIO HTTPS                                    │
 │  S59: Proxy trust CIDR                               │
 │  S68: RSA advisory (russh upstream)                  │
 │  S71: Token expiry policy                            │
 │  S77: DNS rebinding mitigation                       │
 │  S78: Image allowlist                                │
 │  S84: Test namespace NP                              │
 │  S85: Gateway allowedRoutes                          │
 │  S93: Tag name validation                            │
 │  S94: Observability retention                        │
 └──────────────────────────────────────────────────────┘
```

## Test Summary

| Step | Finding | Unit Tests | Integration Tests |
|---|---|---|---|
| 1 | S23/24/57 | — | 3 (default-deny, pipeline-egress, prod-deny) |
| 2 | S31 | — | 3 (no env, volume mount, cleanup) |
| 3 | S36 | — | 3 (requires current, wrong current, admin bypass) |
| 4 | S39 | — | 1 (rate limited) |
| 5 | S43 | — | 1 (audit entry created) |
| 6 | S45 | — | 3 (scoped blocked, scoped allowed, unscoped allowed) |
| 7 | S47 | — | 3 (own ok, others blocked, admin bypass) |
| 8 | S48 | 2 | — |
| 9 | S51+S82 | — | 2 (HSTS conditional, permissions-policy) |
| 10 | S52+S53 | — | 2 (git rate limited, registry rate limited) |
| 11 | S58 | — | — |
| 12 | S61 | — | 1 (generic error) |
| 13 | S63 | — | 1 (SSRF skip) |
| 14 | S64 | — | — |
| 15 | S65 | — | 1 (expiry check) |
| 16 | S66 | — | — |
| 17 | S6 | — | 1 (rolebinding exists) |
| 18 | S70 | — | — (existing tests verify) |
| 19 | S81 | — | — |
| 20 | S89 | — | 1 (null workspace handled) |
| **Total** | | **2** | **~26** |

## Verification

After all steps:
```bash
just test-unit                         # S48, S70
just test-bin auth_integration         # S36, S39
just test-bin secrets_integration      # S43
just test-bin workspace_integration    # S45
just test-bin rbac_integration         # S47
just test-bin observe_integration      # S51, S82
just test-bin git_smart_http_int       # S52
just test-bin registry_integration     # S53
just test-bin merge_request_int        # S61
just test-bin webhook_integration      # S63
just test-bin pipeline_integration     # S31
just ci-full                           # full suite at the end
```
