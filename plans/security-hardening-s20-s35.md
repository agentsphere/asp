# Implementation Plan: Security Hardening S20–S35 (Unique Findings)

**Source:** `plans/security-audit-2026-03-24.md` — UNIQUE findings only (not covered by `/audit` A-prefix or `/audit-ecosystem` E-prefix)
**Scope:** S21, S23, S24, S25, S30, S31, S33, S34, S35 (9 findings)
**Already done:** S20 (workspace owner demotion — implemented in s1-s20 plan)
**Excluded:** S22 (covered A26), S26/S27/S28/S29 (covered E7/E9/E17), S32 (covered A23)

## Implementation Progress

| Step | Finding | Status | Notes |
|---|---|---|---|
| 1 | S21 | ✅ Done | `require_observe_read()` requires admin when no project_id |
| 2 | S23/S24 | ⬜ Deferred | Default-deny NetworkPolicy — needs careful testing with running pipelines. Follow-up. |
| 3 | S25 | ✅ Done | `trustProxy: false` default + auto-derive from `ingress.enabled` |
| 4 | S30 | ✅ Done | `delete_namespace()` checks `platform.io/managed-by` label before delete |
| 5 | S31 | ⬜ Deferred | Git token → K8s Secret volume. Larger refactor (PodSpecParams + executor + cleanup). Follow-up. |
| 6 | S33 | ✅ Done | `host_mount_path` gated on `state.config.dev_mode` |
| 7 | S34 | ✅ Done | `npm ci --ignore-scripts --omit=dev` in Dockerfile.platform-runner |
| 8 | S35 | ✅ Done | `cargo audit` job added to CI workflow |

> **Deviation (Step 6/S33):** Used `state.config.dev_mode` instead of `config.dev_mode` — the local `config` variable is `ProviderConfig` which doesn't have `dev_mode`. `state.config` is the platform `Config` which does.

> **Deferred (S23/S24):** Default-deny NetworkPolicy affects all pods in managed namespaces. Needs careful validation that pipeline pods, deployer pods, and preview pods still function. Better as a dedicated follow-up with targeted E2E testing.

> **Deferred (S31):** Moving git auth token from env var to K8s Secret volume requires changing `PodSpecParams` struct, `build_pod_spec()`, all test call sites (20+), and adding Secret creation/cleanup lifecycle. Better as a dedicated follow-up.

---

## Step 1: S21 — Observe queries require admin when no project_id

**Problem:** `GET /api/observe/logs`, `/traces`, `/metrics` accept optional `project_id`. When `None`, SQL uses `($1::uuid IS NULL OR project_id = $1)` returning ALL projects' data. `require_observe_read()` only checks project access when `project_id` is `Some`.

### 1a. Fix require_observe_read()

**File:** `src/observe/query.rs:212-236`

When `project_id` is `None`, require admin permission instead of allowing any user with ObserveRead to see all projects' data:

```rust
async fn require_observe_read(
    state: &AppState,
    auth: &AuthUser,
    project_id: Option<Uuid>,
) -> Result<(), ApiError> {
    if let Some(pid) = project_id {
        // Scoped query — check project-level observe + project read
        let allowed = resolver::has_permission_scoped(
            &state.pool, &state.valkey, auth.user_id, Some(pid),
            Permission::ObserveRead, auth.token_scopes.as_deref(),
        ).await.map_err(ApiError::Internal)?;
        if !allowed { return Err(ApiError::Forbidden); }
        require_project_read(state, auth, pid).await?;
    } else {
        // Unscoped query (all projects) — require admin
        require_admin(state, auth).await?;
    }
    Ok(())
}
```

### 1b. Tests

**Integration test** (`tests/observe_integration.rs` or equivalent):

```
test_observe_logs_without_project_id_requires_admin:
  1. Create non-admin user with ObserveRead on project A
  2. GET /api/observe/logs (no project_id param)
  3. Assert 403 Forbidden (not admin)

test_observe_logs_without_project_id_admin_ok:
  1. As admin, GET /api/observe/logs (no project_id)
  2. Assert 200

test_observe_logs_with_project_id_non_admin_ok:
  1. Create user with ObserveRead + ProjectRead on project A
  2. GET /api/observe/logs?project_id={A}
  3. Assert 200 (project-scoped query still works)
```

### sqlx changes

None — the function uses dynamic queries.

---

## Step 2: S23 + S24 — Default-deny NetworkPolicy for all managed namespaces

**Problem:** NetworkPolicy only targets `platform.io/component: agent-session` pods. Pipeline pods and user-deployed workloads are unrestricted. No default-deny in project dev/prod namespaces.

### 2a. Add default-deny NetworkPolicy to all managed namespaces

**File:** `src/deployer/namespace.rs` — in `ensure_namespace()` (called for dev, staging, prod, session namespaces), apply a default-deny NetworkPolicy that covers ALL pods:

```rust
// Default-deny NetworkPolicy — blocks all ingress and egress for any pod
// without a more specific allow policy. This is the baseline for all
// platform-managed namespaces.
let default_deny = json!({
    "apiVersion": "networking.k8s.io/v1",
    "kind": "NetworkPolicy",
    "metadata": {
        "name": "default-deny",
        "namespace": ns_name
    },
    "spec": {
        "podSelector": {},  // matches ALL pods
        "policyTypes": ["Ingress", "Egress"]
        // No ingress/egress rules = deny all
    }
});
```

Apply via `apply_namespaced_object()` in `ensure_namespace()`.

### 2b. Add allow policies for platform-managed pods

After the default-deny, add specific allow policies for pods that need network access:

**Pipeline pods** — need DNS + internet egress (for image pulls) + platform API (for status reporting):

```rust
let pipeline_allow = json!({
    "apiVersion": "networking.k8s.io/v1",
    "kind": "NetworkPolicy",
    "metadata": {
        "name": "pipeline-egress",
        "namespace": ns_name
    },
    "spec": {
        "podSelector": {
            "matchLabels": { "platform.io/pipeline": "" }  // exists selector
        },
        "policyTypes": ["Egress"],
        "egress": [
            // DNS
            { "ports": [{"port": 53, "protocol": "UDP"}, {"port": 53, "protocol": "TCP"}] },
            // Platform API (for status callbacks)
            { "to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": platform_namespace}}}],
              "ports": [{"port": 8080, "protocol": "TCP"}] },
            // Internet (image pulls, package downloads) — block cluster-internal CIDRs
            { "to": [{"ipBlock": {"cidr": "0.0.0.0/0", "except": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10"]}}] }
        ]
    }
});
```

The existing `agent-isolation` NetworkPolicy already handles agent-session pods.

### 2c. Apply to project dev/prod namespaces too

Currently `ensure_namespace()` creates the namespace but doesn't apply NetworkPolicy for dev/prod. Add the default-deny there too, gated on `!dev_mode` (same pattern as session namespaces).

**File:** `src/deployer/namespace.rs` — modify `ensure_namespace()` to accept `dev_mode` and `platform_namespace` params, or create a new `ensure_project_namespace()` wrapper.

### 2d. Tests

**Integration test:**

```
test_session_namespace_has_default_deny:
  1. Create session namespace
  2. GET NetworkPolicy "default-deny" in namespace
  3. Assert podSelector is empty (matches all)
  4. Assert policyTypes = ["Ingress", "Egress"]
  5. Assert no ingress/egress rules (deny all)

test_session_namespace_has_pipeline_egress:
  1. Create session namespace
  2. GET NetworkPolicy "pipeline-egress" in namespace
  3. Assert podSelector matches pipeline label
  4. Assert egress includes DNS, platform API, internet (minus cluster CIDRs)

test_project_dev_namespace_has_default_deny:
  1. Create project (triggers dev namespace creation)
  2. GET NetworkPolicy "default-deny" in project-dev namespace
  3. Assert exists
```

---

## Step 3: S25 — Default trustProxy to false in Helm

**Problem:** `helm/platform/values.yaml` defaults `trustProxy: true` but default service type is NodePort (no reverse proxy). Enables IP spoofing.

### 3a. Change default

**File:** `helm/platform/values.yaml:29`

```yaml
# Before:
trustProxy: true

# After:
trustProxy: false
```

### 3b. Auto-derive from ingress.enabled (optional improvement)

**File:** `helm/platform/templates/configmap.yaml:14`

```yaml
# Before:
PLATFORM_TRUST_PROXY: {{ .Values.platform.env.trustProxy | quote }}

# After — auto-enable when ingress is configured:
PLATFORM_TRUST_PROXY: {{ or .Values.platform.env.trustProxy .Values.ingress.enabled | quote }}
```

This way users who enable ingress automatically get trustProxy without needing to set both.

### 3c. Tests

None — Helm values change. Verify with `helm template` or manual inspection.

---

## Step 4: S30 — Guard namespace deletion to platform-managed only

**Problem:** `delete_namespace()` deletes any namespace without checking ownership. A bug or compromise could delete `kube-system`.

### 4a. Add label check before delete

**File:** `src/deployer/namespace.rs:292-308` — replace current `delete_namespace()`:

```rust
pub async fn delete_namespace(kube: &kube::Client, ns_name: &str) -> Result<(), anyhow::Error> {
    let namespaces: kube::Api<k8s_openapi::api::core::v1::Namespace> =
        kube::Api::all(kube.clone());

    // Guard: only delete platform-managed namespaces
    match namespaces.get(ns_name).await {
        Ok(ns) => {
            let labels = ns.metadata.labels.unwrap_or_default();
            if labels.get("platform.io/managed-by").map(String::as_str) != Some("platform") {
                tracing::error!(
                    namespace = %ns_name,
                    "refusing to delete non-platform-managed namespace"
                );
                anyhow::bail!("namespace {ns_name} is not managed by platform");
            }
        }
        Err(kube::Error::Api(err)) if err.code == 404 => {
            tracing::debug!(namespace = %ns_name, "namespace already deleted");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    }

    // Safe to delete — label verified
    namespaces
        .delete(ns_name, &kube::api::DeleteParams::default())
        .await?;

    tracing::info!(namespace = %ns_name, "namespace deleted");
    Ok(())
}
```

### 4b. Tests

**Integration test:**

```
test_delete_namespace_refuses_unmanaged:
  1. Create a namespace WITHOUT platform.io/managed-by label (via kube API directly)
  2. Call delete_namespace()
  3. Assert error "not managed by platform"
  4. Verify namespace still exists

test_delete_namespace_succeeds_for_managed:
  1. Create namespace via ensure_namespace() (has platform.io/managed-by label)
  2. Call delete_namespace()
  3. Assert Ok
  4. Verify namespace is gone (or terminating)

test_delete_namespace_ignores_404:
  1. Call delete_namespace("nonexistent-ns")
  2. Assert Ok (idempotent)
```

---

## Step 5: S31 — Move git auth token from env var to K8s Secret volume

**Problem:** `GIT_AUTH_TOKEN` passed as plain env var in pipeline init container, visible in pod spec to anyone with kubectl access.

### 5a. Create a K8s Secret for the git token

**File:** `src/pipeline/executor.rs` — in `execute_single_step()` (or the calling function), create a K8s Secret containing the git auth token before building the pod spec:

```rust
let git_secret_name = format!("pl-git-{}", &pipeline_id.to_string()[..8]);
let git_secret = Secret {
    metadata: ObjectMeta {
        name: Some(git_secret_name.clone()),
        labels: Some(BTreeMap::from([(
            "platform.io/pipeline".into(),
            pipeline_id.to_string(),
        )])),
        ..Default::default()
    },
    string_data: Some(BTreeMap::from([
        ("token".into(), pipeline.git_auth_token.clone()),
    ])),
    type_: Some("Opaque".into()),
    ..Default::default()
};
let secrets_api: Api<Secret> = Api::namespaced(state.kube.clone(), &meta.namespace);
secrets_api.create(&PostParams::default(), &git_secret).await?;
```

### 5b. Mount Secret as volume in init container

**File:** `src/pipeline/executor.rs:1350-1364` — replace env var with Secret volume mount:

```rust
// Add Secret volume
volumes.push(Volume {
    name: "git-auth".into(),
    secret: Some(SecretVolumeSource {
        secret_name: Some(git_secret_name.clone()),
        ..Default::default()
    }),
    ..Default::default()
});

// Init container reads token from mounted file instead of env var
init_containers: Some(vec![Container {
    name: "clone".into(),
    image: Some("alpine/git:latest".into()),
    command: Some(vec!["sh".into(), "-c".into()]),
    args: Some(vec![format!(
        "printf '#!/bin/sh\\ncat /git-auth/token\\n' > /tmp/git-askpass.sh && \
         chmod +x /tmp/git-askpass.sh && \
         GIT_ASKPASS=/tmp/git-askpass.sh \
         git clone --depth 1 --branch \"$GIT_BRANCH\" {} /workspace 2>&1",
        p.repo_clone_url,
    )]),
    env: Some(vec![env_var("GIT_BRANCH", branch)]),  // no more GIT_AUTH_TOKEN env
    volume_mounts: Some(vec![
        VolumeMount { name: "workspace".into(), mount_path: "/workspace".into(), ..Default::default() },
        VolumeMount { name: "git-auth".into(), mount_path: "/git-auth".into(), read_only: Some(true), ..Default::default() },
    ]),
    security_context: Some(container_security()),
    ..Default::default()
}]),
```

### 5c. Clean up git Secret after pipeline completes

Add cleanup in `cleanup_registry_secret()` (or rename to `cleanup_pipeline_secrets()`):

```rust
// Delete git auth secret
let secret_name = format!("pl-git-{}", &pipeline_id.to_string()[..8]);
let _ = secrets_api.delete(&secret_name, &DeleteParams::default()).await;
```

### 5d. Update PodSpecParams

Remove `git_auth_token` from `PodSpecParams` struct. Pass `git_secret_name` instead. Update all call sites and tests.

### 5e. Tests

**Integration test:**

```
test_pipeline_git_token_not_in_env_vars:
  1. Trigger pipeline
  2. Wait for pod creation
  3. GET pod spec via kube client
  4. Assert init container env does NOT contain GIT_AUTH_TOKEN
  5. Assert a "git-auth" volume mount exists on init container

test_pipeline_git_secret_cleaned_up:
  1. Trigger pipeline, wait for completion
  2. List secrets in namespace matching pl-git-*
  3. Assert none remain
```

---

## Step 6: S33 — Gate host path mount on dev_mode

**Problem:** `PLATFORM_HOST_MOUNT_PATH` env var enables host filesystem mount in agent pods. Not gated on `dev_mode`.

### 6a. Add dev_mode check at the call site

**File:** The calling code where `PodBuildParams` is constructed (in `src/agent/service.rs` or `src/agent/claude_code/mod.rs`). Add:

```rust
// Only allow host mounts in dev mode — production must never mount host paths
let host_mount_path = if config.dev_mode {
    std::env::var("PLATFORM_HOST_MOUNT_PATH").ok().as_deref()
} else {
    if std::env::var("PLATFORM_HOST_MOUNT_PATH").is_ok() {
        tracing::warn!("PLATFORM_HOST_MOUNT_PATH is set but dev_mode is false — ignoring");
    }
    None
};
```

### 6b. Tests

**Unit test:**

```
test_host_mount_ignored_when_not_dev_mode:
  1. Set PLATFORM_HOST_MOUNT_PATH env var
  2. Build PodBuildParams with dev_mode=false
  3. Assert host_mount_path is None

test_host_mount_used_in_dev_mode:
  1. Set PLATFORM_HOST_MOUNT_PATH env var
  2. Build PodBuildParams with dev_mode=true
  3. Assert host_mount_path is Some(...)
```

---

## Step 7: S34 — MCP npm install with --ignore-scripts

**Problem:** `npm install --production` in Dockerfile.platform-runner runs lifecycle scripts from transitive deps.

### 7a. Fix Dockerfile

**File:** `docker/Dockerfile.platform-runner:52`

```dockerfile
# Before:
RUN cd /opt/mcp && npm install --production

# After:
RUN cd /opt/mcp && npm ci --ignore-scripts --omit=dev
```

### 7b. Tests

None — Dockerfile change. Verify by building the image.

---

## Step 8: S35 — Add cargo-audit to CI

**Problem:** No `cargo audit` step in CI. Known CVEs go undetected.

### 8a. Add cargo-audit job to CI

**File:** `.github/workflows/ci.yaml` — add a new job:

```yaml
  audit:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd  # v6
      - uses: dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7  # stable
      - run: cargo install cargo-audit --locked
      - run: cargo audit
```

### 8b. Tests

None — CI configuration. Verify by pushing to a branch.

---

## Execution Order & Dependencies

```
         ┌─────────────────────────────────────┐
         │  Independent — can be parallelized   │
         └─────────────────────────────────────┘

 Step 1 (S21): Observe query admin check       ← query.rs, helpers.rs
 Step 3 (S25): trustProxy default false         ← Helm values only
 Step 4 (S30): Namespace delete guard           ← namespace.rs
 Step 6 (S33): Host mount dev_mode gate         ← agent service.rs / pod.rs
 Step 7 (S34): MCP --ignore-scripts             ← Dockerfile only
 Step 8 (S35): cargo-audit in CI                ← CI config only

         ┌─────────────────────────────────────┐
         │  Depends on namespace.rs changes     │
         └─────────────────────────────────────┘

 Step 2 (S23/S24): Default-deny NetworkPolicy   ← namespace.rs (touches ensure_namespace)
   └── Do after Step 4 (both modify namespace.rs)

         ┌─────────────────────────────────────┐
         │  Larger refactor — executor.rs       │
         └─────────────────────────────────────┘

 Step 5 (S31): Git token Secret volume          ← executor.rs + PodSpecParams change
   └── Touches build_pod_spec which was modified in s1-s20 plan
```

## Test Summary

| Step | Finding | Unit Tests | Integration Tests | E2E Tests |
|---|---|---|---|---|
| 1 | S21 | — | `test_observe_no_project_requires_admin`, `test_observe_no_project_admin_ok`, `test_observe_with_project_ok` | — |
| 2 | S23/S24 | — | `test_session_ns_default_deny`, `test_session_ns_pipeline_egress`, `test_project_dev_ns_default_deny` | — |
| 3 | S25 | — | — | — |
| 4 | S30 | — | `test_delete_ns_refuses_unmanaged`, `test_delete_ns_succeeds_managed`, `test_delete_ns_ignores_404` | — |
| 5 | S31 | — | `test_pipeline_git_token_not_in_env`, `test_pipeline_git_secret_cleaned_up` | — |
| 6 | S33 | `test_host_mount_ignored_not_dev`, `test_host_mount_used_dev` | — | — |
| 7 | S34 | — | — | — |
| 8 | S35 | — | — | — |
| **Total** | | **2 unit** | **~11 integration** | **0 E2E** |

## Verification

After all steps implemented:

```bash
just test-unit                    # Verify unit tests (S33 host mount)
just test-bin observe_integration # Verify S21
just test-bin namespace_test      # Verify S23/S24/S30 (or whichever binary has namespace tests)
just test-bin pipeline_integration # Verify S31
just ci-full                      # Full suite once at the end
```
