// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Gateway auto-deployment controller.
//!
//! Background task that ensures the platform-native ingress gateway is running
//! in the cluster. Creates/updates a `Deployment` + `NodePort` `Service` for the
//! `platform-proxy --gateway` binary.

use std::collections::BTreeMap;
use std::time::Duration;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, HTTPGetAction, Probe, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, DynamicObject, Patch, PatchParams, PostParams};
use kube::discovery::ApiResource;
use tracing::Instrument;

use crate::state::{OperatorConfig, OperatorState};

const COMPONENT_LABEL: &str = "platform.io/component";
const COMPONENT_VALUE: &str = "gateway";
const MANAGED_BY_LABEL: &str = "platform.io/managed-by";
const DEPLOY_NAME: &str = "platform-gateway";
const SA_NAME: &str = "platform-gateway";
const CLUSTER_ROLE_NAME: &str = "platform-gateway";

/// Background task: ensure the platform gateway is running in the cluster.
pub async fn reconcile_gateway(state: OperatorState, cancel: tokio_util::sync::CancellationToken) {
    // Wait for registry seeding to complete before attempting deployment
    tokio::time::sleep(Duration::from_secs(10)).await;

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    state.task_registry.register("gateway_reconciler", 10);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let span = tracing::info_span!("task_iteration",
                    task_name = "gateway_reconciler", source = "system");
                async {
                    match reconcile_once(&state).await {
                        Ok(action) => {
                            state.task_registry.heartbeat("gateway_reconciler");
                            if action != ReconcileAction::NoOp {
                                tracing::info!(?action, "gateway reconciled");
                            }
                        }
                        Err(e) => {
                            state.task_registry.report_error("gateway_reconciler", &e.to_string());
                            tracing::warn!(error = %e, "gateway reconciliation failed");
                        }
                    }
                }
                .instrument(span)
                .await;
            }
            () = cancel.cancelled() => {
                tracing::info!("gateway reconciler shutting down");
                break;
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReconcileAction {
    NoOp,
    Created,
    Updated,
}

pub async fn reconcile_once(state: &OperatorState) -> anyhow::Result<ReconcileAction> {
    let config = &state.config;
    let ns = &config.gateway_namespace;
    let image = resolve_gateway_image(config);

    // Ensure RBAC: ServiceAccount + ClusterRole + per-namespace RoleBindings
    ensure_gateway_rbac(state).await?;

    let deploy_api: Api<Deployment> = Api::namespaced(state.kube.clone(), ns);
    let action = if let Some(existing) = deploy_api.get_opt(DEPLOY_NAME).await? {
        maybe_update_image(&deploy_api, &existing, &image).await?
    } else {
        let deployment = build_deployment(config, &image);
        deploy_api
            .create(&PostParams::default(), &deployment)
            .await?;
        tracing::info!(namespace = %ns, image = %image, "created gateway deployment");
        ReconcileAction::Created
    };

    let svc_api: Api<Service> = Api::namespaced(state.kube.clone(), ns);
    if svc_api.get_opt(DEPLOY_NAME).await?.is_none() {
        let service = build_service(config);
        svc_api.create(&PostParams::default(), &service).await?;
        tracing::info!(namespace = %ns, "created gateway service");
    }

    Ok(action)
}

/// Ensure the gateway `ServiceAccount`, `ClusterRole`, and per-namespace `RoleBinding`s exist.
async fn ensure_gateway_rbac(state: &OperatorState) -> anyhow::Result<()> {
    let config = &state.config;
    let ns = &config.gateway_namespace;
    let pp = PatchParams::apply("platform-gateway-controller").force();

    // 1. ServiceAccount in gateway namespace
    let sa_json = serde_json::json!({
        "apiVersion": "v1",
        "kind": "ServiceAccount",
        "metadata": {
            "name": SA_NAME,
            "namespace": ns
        }
    });
    apply_dynamic(
        &state.kube,
        Some(ns),
        "",
        "v1",
        "ServiceAccount",
        "serviceaccounts",
        SA_NAME,
        sa_json,
        &pp,
    )
    .await?;

    // 2. ClusterRole with read-only access to httproutes + endpointslices + services
    let cr_json = serde_json::json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "ClusterRole",
        "metadata": { "name": CLUSTER_ROLE_NAME },
        "rules": [
            {
                "apiGroups": ["gateway.networking.k8s.io"],
                "resources": ["httproutes"],
                "verbs": ["get", "list", "watch"]
            },
            {
                "apiGroups": ["discovery.k8s.io"],
                "resources": ["endpointslices"],
                "verbs": ["get", "list", "watch"]
            },
            {
                "apiGroups": [""],
                "resources": ["services"],
                "verbs": ["get", "list", "watch"]
            }
        ]
    });
    apply_dynamic(
        &state.kube,
        None,
        "rbac.authorization.k8s.io",
        "v1",
        "ClusterRole",
        "clusterroles",
        CLUSTER_ROLE_NAME,
        cr_json,
        &pp,
    )
    .await?;

    // 3. ClusterRoleBinding -- grants read-only cluster-wide access to httproutes,
    //    endpointslices, and services so the gateway can watch any namespace dynamically.
    let binding_json = serde_json::json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "ClusterRoleBinding",
        "metadata": { "name": CLUSTER_ROLE_NAME },
        "roleRef": {
            "apiGroup": "rbac.authorization.k8s.io",
            "kind": "ClusterRole",
            "name": CLUSTER_ROLE_NAME
        },
        "subjects": [{
            "kind": "ServiceAccount",
            "name": SA_NAME,
            "namespace": ns
        }]
    });
    apply_dynamic(
        &state.kube,
        None,
        "rbac.authorization.k8s.io",
        "v1",
        "ClusterRoleBinding",
        "clusterrolebindings",
        CLUSTER_ROLE_NAME,
        binding_json,
        &pp,
    )
    .await?;

    Ok(())
}

/// Generic server-side apply for a dynamic K8s object.
#[allow(clippy::too_many_arguments)]
async fn apply_dynamic(
    kube_client: &kube::Client,
    namespace: Option<&str>,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    name: &str,
    json_obj: serde_json::Value,
    pp: &PatchParams,
) -> anyhow::Result<()> {
    let api_version = if group.is_empty() {
        version.to_string()
    } else {
        format!("{group}/{version}")
    };
    let ar = ApiResource {
        group: group.into(),
        version: version.into(),
        api_version,
        kind: kind.into(),
        plural: plural.into(),
    };
    let api: Api<DynamicObject> = if let Some(ns) = namespace {
        Api::namespaced_with(kube_client.clone(), ns, &ar)
    } else {
        Api::all_with(kube_client.clone(), &ar)
    };

    let obj: DynamicObject = serde_json::from_value(json_obj)?;
    api.patch(name, pp, &Patch::Apply(&obj)).await?;
    Ok(())
}

async fn maybe_update_image(
    api: &Api<Deployment>,
    existing: &Deployment,
    desired_image: &str,
) -> anyhow::Result<ReconcileAction> {
    let current_image = existing
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.as_ref())
        .map_or("", String::as_str);

    if current_image == desired_image {
        return Ok(ReconcileAction::NoOp);
    }

    tracing::info!(current = %current_image, desired = %desired_image, "updating gateway image");
    let patch = serde_json::json!({
        "spec": { "template": { "spec": { "containers": [{
            "name": "gateway", "image": desired_image,
        }]}}}
    });
    api.patch(
        DEPLOY_NAME,
        &PatchParams::apply("platform-gateway-controller"),
        &Patch::Strategic(patch),
    )
    .await?;
    Ok(ReconcileAction::Updated)
}

fn resolve_gateway_image(config: &OperatorConfig) -> String {
    // Prefer registry_node_url (cluster-internal, e.g. localhost:5001) over
    // registry_url (host-facing, e.g. host.docker.internal:49392) because the
    // kubelet pulls images from inside the node, not from the host network.
    let registry = config
        .registry_node_url
        .as_ref()
        .or(config.registry_url.as_ref());
    if let Some(url) = registry {
        format!("{url}/platform-proxy:v1")
    } else {
        "platform-proxy:v1".into()
    }
}

fn build_deployment(config: &OperatorConfig, image: &str) -> Deployment {
    let ns = &config.gateway_namespace;
    let labels = gateway_labels();
    let container = build_gateway_container(config, image);

    Deployment {
        metadata: ObjectMeta {
            name: Some(DEPLOY_NAME.into()),
            namespace: Some(ns.clone()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: k8s_openapi::api::core::v1::PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    service_account_name: Some(SA_NAME.into()),
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_gateway_container(config: &OperatorConfig, image: &str) -> Container {
    let watch_ns = config.gateway_watch_namespaces.join(",");

    let env = |name: &str, value: String| EnvVar {
        name: name.into(),
        value: Some(value),
        ..Default::default()
    };

    Container {
        name: "gateway".into(),
        image: Some(image.into()),
        args: Some(vec!["--gateway".into()]),
        env: Some(vec![
            env(
                "PROXY_GATEWAY_HTTP_PORT",
                config.gateway_http_port.to_string(),
            ),
            env(
                "PROXY_GATEWAY_TLS_PORT",
                config.gateway_tls_port.to_string(),
            ),
            env("PROXY_GATEWAY_NAME", config.gateway_name.clone()),
            env("PROXY_GATEWAY_NAMESPACE", config.gateway_namespace.clone()),
            env("PROXY_GATEWAY_WATCH_NAMESPACES", watch_ns),
            env("PLATFORM_API_URL", config.platform_api_url.clone()),
            env("PLATFORM_SERVICE_NAME", "platform-gateway".into()),
        ]),
        ports: Some(vec![
            ContainerPort {
                container_port: i32::from(config.gateway_http_port),
                name: Some("http".into()),
                ..Default::default()
            },
            ContainerPort {
                container_port: i32::from(config.gateway_tls_port),
                name: Some("https".into()),
                ..Default::default()
            },
            ContainerPort {
                container_port: 15020,
                name: Some("health".into()),
                ..Default::default()
            },
        ]),
        readiness_probe: Some(health_probe("/readyz", 5)),
        liveness_probe: Some(health_probe("/healthz", 10)),
        ..Default::default()
    }
}

fn health_probe(path: &str, initial_delay: i32) -> Probe {
    Probe {
        http_get: Some(HTTPGetAction {
            path: Some(path.into()),
            port: IntOrString::Int(15020),
            ..Default::default()
        }),
        initial_delay_seconds: Some(initial_delay),
        period_seconds: Some(10),
        ..Default::default()
    }
}

fn build_service(config: &OperatorConfig) -> Service {
    let mk_port = |name: &str, port: u16, node_port: u16| {
        let mut sp = ServicePort {
            name: Some(name.into()),
            port: i32::from(port),
            target_port: Some(IntOrString::Int(i32::from(port))),
            ..Default::default()
        };
        if node_port > 0 {
            sp.node_port = Some(i32::from(node_port));
        }
        sp
    };

    Service {
        metadata: ObjectMeta {
            name: Some(DEPLOY_NAME.into()),
            namespace: Some(config.gateway_namespace.clone()),
            labels: Some(gateway_labels()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("NodePort".into()),
            selector: Some(gateway_labels()),
            ports: Some(vec![
                mk_port(
                    "http",
                    config.gateway_http_port,
                    config.gateway_http_node_port,
                ),
                mk_port(
                    "https",
                    config.gateway_tls_port,
                    config.gateway_tls_node_port,
                ),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn gateway_labels() -> BTreeMap<String, String> {
    BTreeMap::from([
        (COMPONENT_LABEL.into(), COMPONENT_VALUE.into()),
        (MANAGED_BY_LABEL.into(), "platform".into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_gateway_image_with_node_url() {
        let config = test_config(Some("localhost:5001"), Some("host.docker.internal:49392"));
        assert_eq!(
            resolve_gateway_image(&config),
            "localhost:5001/platform-proxy:v1"
        );
    }

    #[test]
    fn resolve_gateway_image_with_registry_url_only() {
        let config = test_config(None, Some("host.docker.internal:49392"));
        assert_eq!(
            resolve_gateway_image(&config),
            "host.docker.internal:49392/platform-proxy:v1"
        );
    }

    #[test]
    fn resolve_gateway_image_no_registry() {
        let config = test_config(None, None);
        assert_eq!(resolve_gateway_image(&config), "platform-proxy:v1");
    }

    #[test]
    fn resolve_gateway_image_prefers_node_url() {
        let config = test_config(Some("node:5001"), Some("host:8080"));
        let image = resolve_gateway_image(&config);
        assert!(
            image.starts_with("node:5001"),
            "should prefer node URL, got: {image}"
        );
    }

    #[test]
    fn gateway_labels_contains_component() {
        let labels = gateway_labels();
        assert_eq!(
            labels.get(COMPONENT_LABEL),
            Some(&COMPONENT_VALUE.to_string())
        );
        assert_eq!(labels.get(MANAGED_BY_LABEL), Some(&"platform".to_string()));
    }

    #[test]
    fn build_deployment_has_correct_metadata() {
        let config = test_config(None, None);
        let deployment = build_deployment(&config, "platform-proxy:v1");
        let meta = &deployment.metadata;
        assert_eq!(meta.name.as_deref(), Some(DEPLOY_NAME));
        assert_eq!(meta.namespace.as_deref(), Some("test-platform"));
    }

    #[test]
    fn build_deployment_has_correct_image() {
        let config = test_config(None, None);
        let deployment = build_deployment(&config, "my-registry/platform-proxy:v1");
        let container = &deployment
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap()
            .containers[0];
        assert_eq!(
            container.image.as_deref(),
            Some("my-registry/platform-proxy:v1")
        );
    }

    #[test]
    fn build_deployment_has_service_account() {
        let config = test_config(None, None);
        let deployment = build_deployment(&config, "platform-proxy:v1");
        let pod_spec = deployment
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap();
        assert_eq!(pod_spec.service_account_name.as_deref(), Some(SA_NAME));
    }

    #[test]
    fn build_gateway_container_has_env_vars() {
        let config = test_config(None, None);
        let container = build_gateway_container(&config, "platform-proxy:v1");
        let env = container.env.as_ref().unwrap();
        let names: Vec<&str> = env.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"PROXY_GATEWAY_HTTP_PORT"));
        assert!(names.contains(&"PROXY_GATEWAY_TLS_PORT"));
        assert!(names.contains(&"PROXY_GATEWAY_NAME"));
        assert!(names.contains(&"PROXY_GATEWAY_NAMESPACE"));
        assert!(names.contains(&"PROXY_GATEWAY_WATCH_NAMESPACES"));
        assert!(names.contains(&"PLATFORM_API_URL"));
        assert!(names.contains(&"PLATFORM_SERVICE_NAME"));
    }

    #[test]
    fn build_gateway_container_has_ports() {
        let config = test_config(None, None);
        let container = build_gateway_container(&config, "platform-proxy:v1");
        let ports = container.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].name.as_deref(), Some("http"));
        assert_eq!(ports[1].name.as_deref(), Some("https"));
        assert_eq!(ports[2].name.as_deref(), Some("health"));
        assert_eq!(ports[2].container_port, 15020);
    }

    #[test]
    fn build_gateway_container_has_probes() {
        let config = test_config(None, None);
        let container = build_gateway_container(&config, "platform-proxy:v1");
        assert!(container.readiness_probe.is_some());
        assert!(container.liveness_probe.is_some());
    }

    #[test]
    fn health_probe_readyz() {
        let probe = health_probe("/readyz", 5);
        let http = probe.http_get.unwrap();
        assert_eq!(http.path.as_deref(), Some("/readyz"));
        assert_eq!(http.port, IntOrString::Int(15020));
        assert_eq!(probe.initial_delay_seconds, Some(5));
        assert_eq!(probe.period_seconds, Some(10));
    }

    #[test]
    fn health_probe_healthz() {
        let probe = health_probe("/healthz", 10);
        let http = probe.http_get.unwrap();
        assert_eq!(http.path.as_deref(), Some("/healthz"));
        assert_eq!(probe.initial_delay_seconds, Some(10));
    }

    #[test]
    fn build_service_has_correct_metadata() {
        let config = test_config(None, None);
        let service = build_service(&config);
        let meta = &service.metadata;
        assert_eq!(meta.name.as_deref(), Some(DEPLOY_NAME));
        assert_eq!(meta.namespace.as_deref(), Some("test-platform"));
    }

    #[test]
    fn build_service_is_nodeport() {
        let config = test_config(None, None);
        let service = build_service(&config);
        let spec = service.spec.as_ref().unwrap();
        assert_eq!(spec.type_.as_deref(), Some("NodePort"));
    }

    #[test]
    fn build_service_has_ports() {
        let config = test_config(None, None);
        let service = build_service(&config);
        let ports = service.spec.as_ref().unwrap().ports.as_ref().unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].name.as_deref(), Some("http"));
        assert_eq!(ports[0].port, 8080);
        assert_eq!(ports[1].name.as_deref(), Some("https"));
        assert_eq!(ports[1].port, 8443);
    }

    #[test]
    fn build_service_omits_zero_node_port() {
        let config = test_config(None, None);
        let service = build_service(&config);
        let ports = service.spec.as_ref().unwrap().ports.as_ref().unwrap();
        // Both node ports are 0 in test config, so node_port should be None
        assert!(ports[0].node_port.is_none());
        assert!(ports[1].node_port.is_none());
    }

    #[test]
    fn build_service_with_nonzero_node_ports() {
        let mut config = test_config(None, None);
        config.gateway_http_node_port = 30080;
        config.gateway_tls_node_port = 30443;
        let service = build_service(&config);
        let ports = service.spec.as_ref().unwrap().ports.as_ref().unwrap();
        assert_eq!(ports[0].node_port, Some(30080));
        assert_eq!(ports[1].node_port, Some(30443));
    }

    #[test]
    fn reconcile_action_equality() {
        assert_eq!(ReconcileAction::NoOp, ReconcileAction::NoOp);
        assert_eq!(ReconcileAction::Created, ReconcileAction::Created);
        assert_eq!(ReconcileAction::Updated, ReconcileAction::Updated);
        assert_ne!(ReconcileAction::NoOp, ReconcileAction::Created);
        assert_ne!(ReconcileAction::Created, ReconcileAction::Updated);
    }

    #[test]
    fn reconcile_action_debug() {
        assert_eq!(format!("{:?}", ReconcileAction::NoOp), "NoOp");
        assert_eq!(format!("{:?}", ReconcileAction::Created), "Created");
        assert_eq!(format!("{:?}", ReconcileAction::Updated), "Updated");
    }

    /// Helper to build a test `OperatorConfig`.
    fn test_config(registry_node_url: Option<&str>, registry_url: Option<&str>) -> OperatorConfig {
        OperatorConfig {
            health_check_interval_secs: 15,
            platform_namespace: "test-platform".into(),
            dev_mode: false,
            master_key: None,
            git_repos_path: std::path::PathBuf::from("/tmp/git-repos"),
            registry_url: registry_url.map(String::from),
            registry_node_url: registry_node_url.map(String::from),
            gateway_name: "platform-gateway".into(),
            gateway_namespace: "test-platform".into(),
            gateway_auto_deploy: false,
            gateway_http_port: 8080,
            gateway_tls_port: 8443,
            gateway_http_node_port: 0,
            gateway_tls_node_port: 0,
            gateway_watch_namespaces: vec![],
            platform_api_url: "http://platform.platform.svc.cluster.local:8080".into(),
        }
    }
}
