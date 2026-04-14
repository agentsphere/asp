// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests that verify pods can actually pull and run images from the
//! platform's built-in OCI registry. Unlike `registry_api.rs` (which tests
//! the registry API), these tests create real K8s pods and verify containerd pulls.
//!
//! Requires `PLATFORM_LISTEN_PORT` (real TCP server for `DaemonSet` proxy to reach)
//! and `PLATFORM_REGISTRY_NODE_URL` (localhost:nodePort for image refs).

mod helpers;

use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, DeleteParams, PostParams};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Wait for a pod to reach a terminal phase (Succeeded or Failed).
/// Returns the phase string.
async fn wait_for_pod(
    kube: &kube::Client,
    namespace: &str,
    name: &str,
    timeout_secs: u64,
) -> String {
    let pods: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed().as_secs() > timeout_secs {
            // Dump pod status for debugging before panicking
            if let Ok(pod) = pods.get(name).await {
                let status = pod.status.as_ref().map(|s| {
                    let phase = s.phase.as_deref().unwrap_or("Unknown");
                    let conditions = s.conditions.as_ref().map(|cs| {
                        cs.iter()
                            .map(|c| format!("{}={}", c.type_, c.status))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                    let container_statuses = s.container_statuses.as_ref().map(|cs| {
                        cs.iter()
                            .map(|c| format!("{}:{:?}", c.name, c.state))
                            .collect::<Vec<_>>()
                            .join("; ")
                    });
                    let init_statuses = s.init_container_statuses.as_ref().map(|cs| {
                        cs.iter()
                            .map(|c| format!("{}:{:?}", c.name, c.state))
                            .collect::<Vec<_>>()
                            .join("; ")
                    });
                    format!(
                        "phase={phase}, conditions=[{}], init=[{}], containers=[{}]",
                        conditions.unwrap_or_default(),
                        init_statuses.unwrap_or_default(),
                        container_statuses.unwrap_or_default(),
                    )
                });
                panic!(
                    "pod {name} did not complete within {timeout_secs}s — status: {}",
                    status.unwrap_or_else(|| "no status".into())
                );
            }
            panic!("pod {name} did not complete within {timeout_secs}s (pod not found)");
        }
        if let Ok(pod) = pods.get(name).await
            && let Some(status) = &pod.status
            && let Some(phase) = &status.phase
            && matches!(phase.as_str(), "Succeeded" | "Failed")
        {
            return phase.clone();
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Build a minimal pod spec that pulls the given image and runs `echo ok`.
fn build_pull_test_pod(name: &str, image: &str, pull_secret_name: Option<&str>) -> Pod {
    use k8s_openapi::api::core::v1::{Container, LocalObjectReference, PodSpec};

    Pod {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(name.into()),
            labels: Some(
                [("platform.io/test".to_string(), "registry-pull".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        },
        spec: Some(PodSpec {
            restart_policy: Some("Never".into()),
            containers: vec![Container {
                name: "test".into(),
                image: Some(image.into()),
                command: Some(vec!["echo".into(), "ok".into()]),
                ..Default::default()
            }],
            image_pull_secrets: pull_secret_name.map(|n| {
                vec![LocalObjectReference {
                    name: n.to_string(),
                }]
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Create a registry pull secret and a pod, wait for completion, then clean up.
/// Returns the pod phase.
async fn run_pull_test(
    state: &platform::store::AppState,
    _admin_token: &str,
    image_name: &str,
    pool: &PgPool,
) -> String {
    let registry_node_url = std::env::var("PLATFORM_REGISTRY_NODE_URL")
        .expect("PLATFORM_REGISTRY_NODE_URL must be set — run via: just test-integration");

    let namespace = &state.config.agent_namespace;
    let pod_name = format!(
        "pull-test-{}-{}",
        image_name,
        &Uuid::new_v4().to_string()[..8]
    );
    let image_ref = format!("{registry_node_url}/{image_name}:v1");
    let label_value = Uuid::new_v4().to_string();

    // Get admin user ID
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(pool)
        .await
        .expect("admin user must exist");

    // Create pull secret
    let pull_secret = platform::registry::pull_secret::create_pull_secret(
        pool,
        &state.kube,
        &registry_node_url,
        admin_id,
        namespace,
        "platform.io/test",
        &label_value,
    )
    .await
    .expect("create pull secret");

    // Create pod
    let pod = build_pull_test_pod(&pod_name, &image_ref, Some(&pull_secret.secret_name));
    let pods: Api<Pod> = Api::namespaced(state.kube.clone(), namespace);
    pods.create(&PostParams::default(), &pod)
        .await
        .expect("create pod");

    // Wait for pod to complete (120s — image pull can be slow)
    let phase = wait_for_pod(&state.kube, namespace, &pod_name, 120).await;

    // Cleanup
    let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
    platform::registry::pull_secret::cleanup_pull_secret(
        pool,
        &state.kube,
        &pull_secret.secret_name,
        &pull_secret.token_hash,
        namespace,
    )
    .await;

    phase
}

// ---------------------------------------------------------------------------
// Tests — serialized via nextest test-group (share PLATFORM_LISTEN_PORT)
// ---------------------------------------------------------------------------

/// Verify that a pod can pull and run the `platform-runner` image from the
/// built-in registry. This image is the full agent runtime (Node.js + Claude CLI
/// + agent-runner + MCP servers).
#[sqlx::test(migrations = "./migrations")]
async fn pull_platform_runner_image(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_test_server(pool.clone()).await;

    let phase = run_pull_test(&state, &admin_token, "platform-runner", &pool).await;
    assert_eq!(phase, "Succeeded", "platform-runner pod should succeed");
}

/// Verify that a pod can pull and run the `platform-runner-bare` image from the
/// built-in registry. This is the minimal image (Node.js + git + curl) used for
/// auto-setup flows where tools are installed at pod start.
#[sqlx::test(migrations = "./migrations")]
async fn pull_platform_runner_bare_image(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_test_server(pool.clone()).await;

    let phase = run_pull_test(&state, &admin_token, "platform-runner-bare", &pool).await;
    assert_eq!(
        phase, "Succeeded",
        "platform-runner-bare pod should succeed"
    );
}

/// Verify that the auto-setup init container can download agent-runner from the
/// platform server's `/api/downloads/agent-runner` endpoint. Uses the bare image
/// with a curl-based download script (same mechanism as `build_setup_tools_container`).
#[sqlx::test(migrations = "./migrations")]
async fn auto_setup_downloads_agent_runner(pool: PgPool) {
    use k8s_openapi::api::core::v1::{
        Container, EmptyDirVolumeSource, PodSpec, Volume, VolumeMount,
    };

    let (state, admin_token, _server) = helpers::start_test_server(pool.clone()).await;

    // 1. Verify pre-built agent-runner binaries exist (built by `just cli-cross`)
    let runner_dir = &state.config.agent_runner_dir;
    assert!(
        runner_dir.join("arm64").exists() && runner_dir.join("amd64").exists(),
        "agent-runner binaries not found at {} — run `just cli-cross` first",
        runner_dir.display()
    );

    // 2. Get env vars for in-cluster networking
    let registry_node_url = std::env::var("PLATFORM_REGISTRY_NODE_URL")
        .expect("PLATFORM_REGISTRY_NODE_URL must be set");
    let platform_api_url = std::env::var("PLATFORM_API_URL").expect("PLATFORM_API_URL must be set");
    let namespace = &state.config.agent_namespace;
    let pod_name = format!("setup-test-{}", &Uuid::new_v4().to_string()[..8]);
    let image = format!("{registry_node_url}/platform-runner-bare:v1");

    // 3. Get admin user ID for pull secret
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&pool)
        .await
        .expect("admin user must exist");

    // Create pull secret for the bare image
    let label_value = Uuid::new_v4().to_string();
    let pull_secret = platform::registry::pull_secret::create_pull_secret(
        &pool,
        &state.kube,
        &registry_node_url,
        admin_id,
        namespace,
        "platform.io/test",
        &label_value,
    )
    .await
    .expect("create pull secret");

    // Shared workspace volume between init and main containers
    let workspace_vol = Volume {
        name: "workspace".into(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Default::default()
    };
    let workspace_mount = VolumeMount {
        name: "workspace".into(),
        mount_path: "/workspace".into(),
        ..Default::default()
    };

    // Init container: download agent-runner via curl (same as production setup script)
    let download_script = format!(
        r#"set -eu
BIN_DIR=/workspace/.platform/bin
mkdir -p "$BIN_DIR"
ARCH=$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
echo "[setup] Downloading agent-runner ($ARCH) from {platform_api_url}..."
curl -sf -H "Authorization: Bearer {admin_token}" \
  "{platform_api_url}/api/downloads/agent-runner?arch=${{ARCH}}" \
  -o "$BIN_DIR/agent-runner"
chmod +x "$BIN_DIR/agent-runner"
echo "[setup] agent-runner downloaded and made executable""#,
    );

    let init_container = Container {
        name: "setup-tools".into(),
        image: Some(image.clone()),
        command: Some(vec!["sh".into(), "-c".into()]),
        args: Some(vec![download_script]),
        volume_mounts: Some(vec![workspace_mount.clone()]),
        ..Default::default()
    };

    // Main container: verify the binary exists and is executable
    let main_container = Container {
        name: "verify".into(),
        image: Some(image.clone()),
        command: Some(vec!["sh".into(), "-c".into()]),
        args: Some(vec![
            "test -x /workspace/.platform/bin/agent-runner && echo ok".into(),
        ]),
        volume_mounts: Some(vec![workspace_mount]),
        ..Default::default()
    };

    let pod = Pod {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(pod_name.clone()),
            labels: Some(
                [("platform.io/test".to_string(), "auto-setup".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        },
        spec: Some(PodSpec {
            restart_policy: Some("Never".into()),
            init_containers: Some(vec![init_container]),
            containers: vec![main_container],
            volumes: Some(vec![workspace_vol]),
            image_pull_secrets: Some(vec![k8s_openapi::api::core::v1::LocalObjectReference {
                name: pull_secret.secret_name.clone(),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    // 4. Create pod and wait for completion
    let pods: Api<Pod> = Api::namespaced(state.kube.clone(), namespace);
    pods.create(&PostParams::default(), &pod)
        .await
        .expect("create setup-test pod");

    let phase = wait_for_pod(&state.kube, namespace, &pod_name, 120).await;

    // 5. Cleanup
    let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
    platform::registry::pull_secret::cleanup_pull_secret(
        &pool,
        &state.kube,
        &pull_secret.secret_name,
        &pull_secret.token_hash,
        namespace,
    )
    .await;

    assert_eq!(
        phase, "Succeeded",
        "auto-setup pod should download agent-runner and succeed"
    );
}
