// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Operator subsystem state.

use std::sync::Arc;

use platform_types::health::TaskRegistry;
use sqlx::PgPool;

use crate::health::HealthSnapshot;

/// Operator-specific configuration.
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// Health check interval in seconds (default 15).
    pub health_check_interval_secs: u64,
    /// Platform namespace.
    pub platform_namespace: String,
    /// Dev mode flag.
    pub dev_mode: bool,
    /// Master key for secrets engine health check.
    pub master_key: Option<String>,
    /// Path to git repositories (health check).
    pub git_repos_path: std::path::PathBuf,
    /// Registry URL for gateway image resolution.
    pub registry_url: Option<String>,
    /// Node-accessible registry URL (preferred over `registry_url` for kubelet pulls).
    pub registry_node_url: Option<String>,
    /// Name of the shared Gateway resource (default "platform-gateway").
    pub gateway_name: String,
    /// Namespace where the shared Gateway lives.
    pub gateway_namespace: String,
    /// Whether to auto-deploy the gateway.
    pub gateway_auto_deploy: bool,
    /// Gateway HTTP listen port inside the pod (default 8080).
    pub gateway_http_port: u16,
    /// Gateway TLS listen port inside the pod (default 8443).
    pub gateway_tls_port: u16,
    /// `NodePort` for gateway HTTP (0 = K8s auto-assign).
    pub gateway_http_node_port: u16,
    /// `NodePort` for gateway TLS (0 = K8s auto-assign).
    pub gateway_tls_node_port: u16,
    /// Namespaces the gateway should watch. Empty = all labeled.
    pub gateway_watch_namespaces: Vec<String>,
    /// Platform API URL for gateway pods to reach the platform.
    pub platform_api_url: String,
}

/// Shared state for the operator subsystem.
#[derive(Clone)]
pub struct OperatorState {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub kube: kube::Client,
    pub minio: opendal::Operator,
    pub config: Arc<OperatorConfig>,
    pub task_registry: Arc<TaskRegistry>,
    pub health: Arc<std::sync::RwLock<HealthSnapshot>>,
}
