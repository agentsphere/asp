// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! K8s integration tests for `platform-k8s` crate.
//!
//! All tests are `#[ignore = "requires K8s"]` — run via `just crate-test-kubernetes`.
//! Requires a Kind cluster (`just cluster-up`).

use k8s_openapi::api::core::v1::{ConfigMap, LimitRange, Namespace, ResourceQuota, ServiceAccount};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::api::rbac::v1::{Role, RoleBinding};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_ns() -> String {
    format!("k8s-crate-test-{}", &Uuid::new_v4().to_string()[..8])
}

async fn kube_client() -> kube::Client {
    kube::Client::try_default()
        .await
        .expect("KUBECONFIG must be set for K8s tests")
}

/// Force-delete a namespace. Ignores 404.
async fn force_cleanup(kube: &kube::Client, ns: &str) {
    let api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    match api.delete(ns, &kube::api::DeleteParams::default()).await {
        Ok(_) | Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {}
        Err(e) => tracing::warn!(error = %e, ns, "cleanup failed"),
    }
}

/// Wait until namespace exists (up to 5s).
async fn wait_ns_ready(kube: &kube::Client, ns: &str) {
    let api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    for _ in 0..50 {
        if api.get(ns).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("namespace {ns} did not become ready in 5s");
}

// ---------------------------------------------------------------------------
// ensure_namespace
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_namespace_creates_resources() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    let result = platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await;
    assert!(result.is_ok(), "ensure_namespace failed: {result:?}");

    wait_ns_ready(&kube, &ns).await;

    // Verify namespace labels
    let ns_api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    let ns_obj = ns_api.get(&ns).await.expect("namespace should exist");
    let labels = ns_obj.metadata.labels.as_ref().unwrap();
    assert_eq!(labels.get("platform.io/managed-by").unwrap(), "platform");
    assert_eq!(labels.get("platform.io/env").unwrap(), "dev");
    assert_eq!(labels.get("platform.io/project").unwrap(), &project_id);

    // Verify RoleBindings
    let rb_api: kube::Api<RoleBinding> = kube::Api::namespaced(kube.clone(), &ns);
    let secrets_rb = rb_api
        .get("platform-secrets-access")
        .await
        .expect("secrets RoleBinding should exist");
    assert_eq!(
        secrets_rb.role_ref.name, "platform-secrets-manager",
        "secrets RB should ref platform-secrets-manager"
    );

    let gateway_rb = rb_api
        .get("platform-gateway-access")
        .await
        .expect("gateway RoleBinding should exist");
    assert_eq!(gateway_rb.role_ref.name, "platform-gateway");

    // Verify NetworkPolicy
    let np_api: kube::Api<NetworkPolicy> = kube::Api::namespaced(kube.clone(), &ns);
    let np = np_api
        .get("platform-managed")
        .await
        .expect("NetworkPolicy should exist");
    assert!(np.spec.is_some());

    force_cleanup(&kube, &ns).await;
}

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_namespace_idempotent() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    // Call twice — second call should not error
    platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await
    .expect("first call");

    let result = platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await;
    assert!(result.is_ok(), "idempotent second call failed: {result:?}");

    force_cleanup(&kube, &ns).await;
}

// ---------------------------------------------------------------------------
// ensure_namespace_with_services_ns
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_namespace_with_services_ns_creates_resources() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    let result = platform_k8s::ensure_namespace_with_services_ns(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        "services-ns",
        false,
    )
    .await;
    assert!(
        result.is_ok(),
        "ensure_namespace_with_services_ns failed: {result:?}"
    );

    wait_ns_ready(&kube, &ns).await;

    // Verify namespace exists with correct labels
    let ns_api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    let ns_obj = ns_api.get(&ns).await.expect("namespace should exist");
    let labels = ns_obj.metadata.labels.as_ref().unwrap();
    assert_eq!(labels.get("platform.io/managed-by").unwrap(), "platform");

    force_cleanup(&kube, &ns).await;
}

// ---------------------------------------------------------------------------
// ensure_session_namespace
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_session_namespace_creates_all_resources() {
    let kube = kube_client().await;
    let ns = test_ns();
    let session_id = Uuid::new_v4().to_string();
    let project_id = Uuid::new_v4().to_string();

    let result = platform_k8s::ensure_session_namespace(
        &kube,
        &ns,
        &session_id,
        &project_id,
        "platform",
        "platform",
        None,
        false,
    )
    .await;
    assert!(
        result.is_ok(),
        "ensure_session_namespace failed: {result:?}"
    );

    wait_ns_ready(&kube, &ns).await;

    // Verify PSA labels (session + non-dev → baseline enforcement)
    let ns_api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    let ns_obj = ns_api.get(&ns).await.expect("namespace should exist");
    let labels = ns_obj.metadata.labels.as_ref().unwrap();
    assert_eq!(
        labels
            .get("pod-security.kubernetes.io/enforce")
            .map(String::as_str),
        Some("baseline"),
        "session namespace should have PSA baseline enforcement"
    );

    // Verify NetworkPolicy (agent-isolation)
    let np_api: kube::Api<NetworkPolicy> = kube::Api::namespaced(kube.clone(), &ns);
    np_api
        .get("agent-isolation")
        .await
        .expect("session NetworkPolicy should exist");

    // Verify ServiceAccount
    let sa_api: kube::Api<ServiceAccount> = kube::Api::namespaced(kube.clone(), &ns);
    sa_api
        .get("agent-sa")
        .await
        .expect("ServiceAccount should exist");

    // Verify Role
    let role_api: kube::Api<Role> = kube::Api::namespaced(kube.clone(), &ns);
    let role = role_api.get("agent-edit").await.expect("Role should exist");
    assert!(role.rules.is_some());

    // Verify RoleBinding
    let rb_api: kube::Api<RoleBinding> = kube::Api::namespaced(kube.clone(), &ns);
    let rb = rb_api
        .get("agent-edit-binding")
        .await
        .expect("RoleBinding should exist");
    assert_eq!(rb.role_ref.name, "agent-edit");

    // Verify ResourceQuota
    let quota_api: kube::Api<ResourceQuota> = kube::Api::namespaced(kube.clone(), &ns);
    quota_api
        .get("session-quota")
        .await
        .expect("ResourceQuota should exist");

    // Verify LimitRange
    let lr_api: kube::Api<LimitRange> = kube::Api::namespaced(kube.clone(), &ns);
    lr_api
        .get("session-limits")
        .await
        .expect("LimitRange should exist");

    force_cleanup(&kube, &ns).await;
}

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_session_namespace_dev_mode_skips_network_policy() {
    let kube = kube_client().await;
    let ns = test_ns();
    let session_id = Uuid::new_v4().to_string();
    let project_id = Uuid::new_v4().to_string();

    platform_k8s::ensure_session_namespace(
        &kube,
        &ns,
        &session_id,
        &project_id,
        "platform",
        "platform",
        None,
        true, // dev_mode = true
    )
    .await
    .expect("ensure_session_namespace dev_mode");

    wait_ns_ready(&kube, &ns).await;

    // Verify no PSA labels
    let ns_api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    let ns_obj = ns_api.get(&ns).await.expect("namespace should exist");
    let labels = ns_obj.metadata.labels.as_ref().unwrap();
    assert!(
        labels.get("pod-security.kubernetes.io/enforce").is_none(),
        "dev_mode session namespace should NOT have PSA labels"
    );

    // Verify no agent-isolation NetworkPolicy
    let np_api: kube::Api<NetworkPolicy> = kube::Api::namespaced(kube.clone(), &ns);
    let result = np_api.get("agent-isolation").await;
    assert!(
        result.is_err(),
        "dev_mode should NOT create agent-isolation NetworkPolicy"
    );

    // But RBAC should still exist
    let sa_api: kube::Api<ServiceAccount> = kube::Api::namespaced(kube.clone(), &ns);
    sa_api
        .get("agent-sa")
        .await
        .expect("ServiceAccount should still exist in dev_mode");

    force_cleanup(&kube, &ns).await;
}

// ---------------------------------------------------------------------------
// delete_namespace
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn delete_namespace_removes_managed_namespace() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    // Create a managed namespace
    platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await
    .expect("create namespace");

    wait_ns_ready(&kube, &ns).await;

    // Delete it
    let result = platform_k8s::delete_namespace(&kube, &ns).await;
    assert!(result.is_ok(), "delete_namespace failed: {result:?}");

    // Give K8s a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Namespace should be gone or terminating
    let ns_api: kube::Api<Namespace> = kube::Api::all(kube.clone());
    match ns_api.get(&ns).await {
        Err(kube::Error::Api(e)) if e.code == 404 => {} // gone
        Ok(ns_obj) => {
            // Still exists but may be terminating
            let phase = ns_obj.status.as_ref().and_then(|s| s.phase.as_deref());
            assert_eq!(
                phase,
                Some("Terminating"),
                "namespace should be terminating, got: {phase:?}"
            );
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[tokio::test]
#[ignore = "requires K8s"]
async fn delete_namespace_nonexistent_returns_ok() {
    let kube = kube_client().await;
    let ns = format!(
        "k8s-crate-test-nonexist-{}",
        &Uuid::new_v4().to_string()[..8]
    );

    // Deleting a namespace that doesn't exist should be Ok (idempotent)
    let result = platform_k8s::delete_namespace(&kube, &ns).await;
    assert!(
        result.is_ok(),
        "deleting nonexistent namespace should be Ok: {result:?}"
    );
}

#[tokio::test]
#[ignore = "requires K8s"]
async fn delete_namespace_refuses_unmanaged() {
    let kube = kube_client().await;

    // `kube-system` is not managed by platform — should be refused
    let result = platform_k8s::delete_namespace(&kube, "kube-system").await;
    assert!(
        result.is_err(),
        "should refuse to delete unmanaged namespace"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("managed-by"),
        "error should mention managed-by label, got: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// ensure_mesh_ca_bundle
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_mesh_ca_bundle_creates_configmap() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    // Create the namespace first
    platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await
    .expect("create namespace");

    wait_ns_ready(&kube, &ns).await;

    // Create mesh CA bundle
    let ca_pem = "-----BEGIN CERTIFICATE-----\ntest-ca-data\n-----END CERTIFICATE-----";
    let result = platform_k8s::ensure_mesh_ca_bundle(&kube, &ns, ca_pem).await;
    assert!(result.is_ok(), "ensure_mesh_ca_bundle failed: {result:?}");

    // Verify ConfigMap
    let cm_api: kube::Api<ConfigMap> = kube::Api::namespaced(kube.clone(), &ns);
    let cm = cm_api
        .get("mesh-ca-bundle")
        .await
        .expect("ConfigMap should exist");
    let data = cm.data.as_ref().expect("ConfigMap should have data");
    assert_eq!(data.get("ca.pem").unwrap(), ca_pem);

    force_cleanup(&kube, &ns).await;
}

// ---------------------------------------------------------------------------
// ensure_network_policy
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_network_policy_creates_policy() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    // Create namespace first
    platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await
    .expect("create namespace");

    wait_ns_ready(&kube, &ns).await;

    // Apply agent-isolation network policy
    let result = platform_k8s::ensure_network_policy(&kube, &ns, "platform").await;
    assert!(result.is_ok(), "ensure_network_policy failed: {result:?}");

    // Verify NetworkPolicy
    let np_api: kube::Api<NetworkPolicy> = kube::Api::namespaced(kube.clone(), &ns);
    let np = np_api
        .get("agent-isolation")
        .await
        .expect("NetworkPolicy should exist");
    let spec = np.spec.as_ref().unwrap();
    assert!(spec.egress.is_some());
    assert!(spec.ingress.is_some());

    force_cleanup(&kube, &ns).await;
}

// ---------------------------------------------------------------------------
// ensure_session_network_policy
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires K8s"]
async fn ensure_session_network_policy_creates_policy() {
    let kube = kube_client().await;
    let ns = test_ns();
    let project_id = Uuid::new_v4().to_string();

    // Create namespace first
    platform_k8s::ensure_namespace(
        &kube,
        &ns,
        "dev",
        &project_id,
        "platform",
        "platform",
        false,
    )
    .await
    .expect("create namespace");

    wait_ns_ready(&kube, &ns).await;

    // Apply session network policy
    let result =
        platform_k8s::ensure_session_network_policy(&kube, &ns, "platform", "platform").await;
    assert!(
        result.is_ok(),
        "ensure_session_network_policy failed: {result:?}"
    );

    // Verify NetworkPolicy exists with session-specific config
    let np_api: kube::Api<NetworkPolicy> = kube::Api::namespaced(kube.clone(), &ns);
    let np = np_api
        .get("agent-isolation")
        .await
        .expect("session NetworkPolicy should exist");
    let spec = np.spec.as_ref().unwrap();

    // Session policy has 2 ingress rules (platform + mTLS)
    let ingress = spec.ingress.as_ref().unwrap();
    assert_eq!(
        ingress.len(),
        2,
        "session policy should have 2 ingress rules"
    );

    force_cleanup(&kube, &ns).await;
}
