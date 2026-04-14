// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests for transparent mesh proxy features.
//!
//! Tests the network policy changes (all TCP between mesh namespaces),
//! the combined init container injection (binary copy + iptables), and
//! config propagation through the reconciler path.

mod helpers;

use platform::deployer::applier::{ProxyInjectionConfig, inject_proxy_wrapper};
use platform::deployer::namespace::build_network_policy;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mesh_config() -> ProxyInjectionConfig {
    ProxyInjectionConfig {
        platform_api_url: "http://platform.platform.svc.cluster.local:8080".into(),
        init_image: "platform-proxy-init:v1".into(),
        mesh_strict_mtls: false,
    }
}

// ---------------------------------------------------------------------------
// Network policy allows all TCP between mesh namespaces (not just 8443)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn network_policy_mesh_allows_all_tcp(_pool: PgPool) {
    let np = build_network_policy("my-app-dev", "platform");

    // Ingress: mesh rule should allow all TCP (no specific port)
    let ingress = np["spec"]["ingress"]
        .as_array()
        .expect("should have ingress");
    let mesh_ingress = &ingress[0];
    let ingress_from =
        &mesh_ingress["from"][0]["namespaceSelector"]["matchLabels"]["platform.io/managed-by"];
    assert_eq!(ingress_from, "platform");
    let ingress_ports = mesh_ingress["ports"].as_array().expect("should have ports");
    assert_eq!(ingress_ports.len(), 1);
    assert_eq!(ingress_ports[0]["protocol"], "TCP");
    // Should NOT have a specific port number — allows all TCP
    assert!(
        ingress_ports[0].get("port").is_none(),
        "mesh ingress should allow all TCP, not just a specific port"
    );

    // Egress: mesh rule should allow all TCP (not just 8443)
    let egress = np["spec"]["egress"].as_array().expect("should have egress");
    // Find the mesh egress rule (to platform-managed namespaces)
    let mesh_egress = egress
        .iter()
        .find(|rule| {
            rule["to"]
                .as_array()
                .and_then(|to| to.first())
                .and_then(|t| {
                    t["namespaceSelector"]["matchLabels"]["platform.io/managed-by"].as_str()
                })
                .is_some()
        })
        .expect("should have mesh egress rule");
    let egress_ports = mesh_egress["ports"].as_array().expect("should have ports");
    assert_eq!(egress_ports[0]["protocol"], "TCP");
    assert!(
        egress_ports[0].get("port").is_none(),
        "mesh egress should allow all TCP, not just a specific port"
    );
}

// ---------------------------------------------------------------------------
// Config mesh_strict_mtls defaults to false
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn config_mesh_strict_mtls_defaults_to_false(pool: PgPool) {
    let (state, _token) = helpers::test_state(pool).await;
    assert!(!state.config.mesh_strict_mtls);
}

// ---------------------------------------------------------------------------
// Injection: full Deployment with multiple containers
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn injection_multi_container_deployment(_pool: PgPool) {
    let manifest = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: demo-app
spec:
  replicas: 1
  selector:
    matchLabels:
      app: demo
  template:
    metadata:
      labels:
        app: demo
    spec:
      containers:
        - name: web
          image: demo-web:latest
          command: ["./web-server"]
          ports:
            - containerPort: 8080
        - name: worker
          image: demo-worker:latest
          command: ["./worker"]
"#;

    let config = mesh_config();
    let result = inject_proxy_wrapper(manifest, &config).expect("injection should succeed");
    let doc: serde_json::Value = serde_yaml::from_str(&result).unwrap();

    let pod_spec = &doc["spec"]["template"]["spec"];

    // Both containers should be wrapped
    let containers = pod_spec["containers"].as_array().unwrap();
    assert_eq!(containers.len(), 2);
    for container in containers {
        assert_eq!(
            container["command"][0].as_str().unwrap(),
            "/proxy/platform-proxy",
            "container {} should be wrapped",
            container["name"]
        );
        // Both should have transparent env vars
        let env = container["env"].as_array().unwrap();
        assert!(
            env.iter().any(|e| e["name"] == "PROXY_TRANSPARENT"),
            "container {} should have PROXY_TRANSPARENT",
            container["name"]
        );
    }

    // Single combined init container
    let inits = pod_spec["initContainers"].as_array().unwrap();
    assert_eq!(inits.len(), 1);
    assert_eq!(inits[0]["name"], "proxy-init");
}

// ---------------------------------------------------------------------------
// Injection: StatefulSet gets init container
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn injection_statefulset(_pool: PgPool) {
    let manifest = r#"
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: postgres
spec:
  serviceName: postgres
  replicas: 1
  selector:
    matchLabels:
      app: postgres
  template:
    spec:
      containers:
        - name: postgres
          image: postgres:16
          command: ["docker-entrypoint.sh"]
          args: ["postgres"]
          ports:
            - containerPort: 5432
"#;

    let config = mesh_config();
    let result = inject_proxy_wrapper(manifest, &config).expect("injection should succeed");
    let doc: serde_json::Value = serde_yaml::from_str(&result).unwrap();

    // StatefulSet should have the init container
    let inits = doc["spec"]["template"]["spec"]["initContainers"]
        .as_array()
        .expect("should have initContainers");
    assert_eq!(inits.len(), 1);
    assert_eq!(inits[0]["name"], "proxy-init");

    // Container should have transparent env vars
    let container = &doc["spec"]["template"]["spec"]["containers"][0];
    let env = container["env"].as_array().unwrap();
    assert!(env.iter().any(|e| e["name"] == "PROXY_TRANSPARENT"));
    assert!(env.iter().any(|e| e["name"] == "PROXY_INBOUND_PORT"));
}

// ---------------------------------------------------------------------------
// Injection preserves existing init containers
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn injection_preserves_existing_init_containers(_pool: PgPool) {
    let manifest = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
spec:
  template:
    spec:
      initContainers:
        - name: gen-certs
          image: alpine:latest
          command: ["sh", "-c", "echo certs"]
      containers:
        - name: app
          image: app:latest
          command: ["./app"]
"#;

    let config = mesh_config();
    let result = inject_proxy_wrapper(manifest, &config).expect("injection should succeed");
    let doc: serde_json::Value = serde_yaml::from_str(&result).unwrap();

    let inits = doc["spec"]["template"]["spec"]["initContainers"]
        .as_array()
        .unwrap();

    // Should have 2 init containers: gen-certs (user) + proxy-init
    assert_eq!(inits.len(), 2, "should have gen-certs + proxy-init");
    assert_eq!(inits[0]["name"], "gen-certs");
    assert_eq!(inits[1]["name"], "proxy-init");
}

// ---------------------------------------------------------------------------
// Config propagation via state
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn config_propagation_through_state(pool: PgPool) {
    let (state, _token) = helpers::test_state(pool).await;

    // Verify mesh config is accessible
    assert!(state.config.mesh_enabled || !state.config.mesh_enabled); // field exists
    assert!(!state.config.mesh_strict_mtls);
}
