// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Deployer subsystem state — no dependency on the main binary's `AppState`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platform_types::traits::TaskHeartbeat;
use sqlx::PgPool;
use uuid::Uuid;

/// External service dependencies for the reconciler loop.
///
/// Concrete implementations are provided by the binary; tests can mock.
pub trait ReconcilerServices: Send + Sync + Clone + 'static {
    /// Fire webhooks for a project event.
    fn fire_webhooks(
        &self,
        project_id: Uuid,
        event: &str,
        payload: &serde_json::Value,
    ) -> impl Future<Output = ()> + Send;

    /// Render Kustomize/Helm manifests with variables and apply to a K8s namespace.
    fn render_and_apply(
        &self,
        kube: &kube::Client,
        manifest: &str,
        vars: &serde_json::Value,
        ns: &str,
        tracking: Option<&str>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Read a file from an ops repo at a given git ref.
    fn ops_read_file(
        &self,
        repo_path: &Path,
        git_ref: &str,
        file: &str,
    ) -> impl Future<Output = Option<String>> + Send;

    /// Read all YAML files in a directory from an ops repo.
    fn ops_read_dir_yaml(
        &self,
        repo_path: &Path,
        git_ref: &str,
        dir: &str,
    ) -> impl Future<Output = Option<String>> + Send;

    /// Commit key=value pairs to an ops repo.
    fn ops_commit_values(
        &self,
        ops_path: &Path,
        branch: &str,
        values: &[(&str, &str)],
        msg: &str,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;

    /// Read the project secret value for injection.
    fn decrypt_secret(
        &self,
        project_id: Uuid,
        key: &str,
    ) -> impl Future<Output = anyhow::Result<Option<String>>> + Send;

    /// Ensure a K8s namespace exists with the proper labels and network policies.
    #[allow(clippy::too_many_arguments)]
    fn ensure_namespace(
        &self,
        kube: &kube::Client,
        ns_name: &str,
        env: &str,
        project_id: &str,
        platform_namespace: &str,
        gateway_namespace: &str,
        dev_mode: bool,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Delete a K8s namespace.
    fn delete_namespace(
        &self,
        kube: &kube::Client,
        ns_name: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Sync an ops repo and return `(repo_path, sha, branch)`.
    fn sync_ops_repo(
        &self,
        pool: &PgPool,
        ops_repo_id: Uuid,
    ) -> impl Future<Output = anyhow::Result<(PathBuf, String, String)>> + Send;

    /// Get the SHA of a branch in an ops repo.
    fn get_branch_sha(
        &self,
        repo_path: &Path,
        branch: &str,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;

    /// Get the HEAD SHA of an ops repo.
    fn get_head_sha(&self, repo_path: &Path)
    -> impl Future<Output = anyhow::Result<String>> + Send;

    /// Read environment-specific values from an ops repo.
    fn read_values(
        &self,
        repo_path: &Path,
        branch: &str,
        environment: &str,
    ) -> impl Future<Output = anyhow::Result<serde_json::Value>> + Send;

    /// Generate an API token, returning `(raw_token, hash)`.
    fn generate_api_token(&self) -> (String, String);

    /// Publish an event to the platform event bus.
    fn publish_event(
        &self,
        valkey: &fred::clients::Pool,
        event_json: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Check an alert condition (e.g. "gt", "lt") against a threshold and value.
    fn check_condition(&self, condition: &str, threshold: Option<f64>, value: Option<f64>) -> bool;

    /// Evaluate a metric from the observe store.
    fn evaluate_metric(
        &self,
        pool: &PgPool,
        name: &str,
        labels: Option<&serde_json::Value>,
        agg: &str,
        window_secs: i32,
    ) -> impl Future<Output = anyhow::Result<Option<f64>>> + Send;
}

/// Deployer configuration subset.
#[derive(Debug, Clone)]
pub struct DeployerConfig {
    /// Path to ops repo storage.
    pub ops_repos_path: String,
    /// Platform namespace for network policies.
    pub platform_namespace: String,
    /// Namespace prefix for per-project namespaces.
    pub ns_prefix: Option<String>,
    /// Dev mode flag.
    pub dev_mode: bool,
    /// Gateway namespace.
    pub gateway_namespace: String,
    /// Preview proxy URL (dev only).
    pub preview_proxy_url: Option<String>,
    /// Node-accessible registry URL.
    pub registry_node_url: Option<String>,
    /// Registry URL.
    pub registry_url: Option<String>,
    /// Proxy binary path (dev only).
    pub proxy_binary_path: Option<String>,
    /// Whether service mesh (proxy injection) is enabled.
    pub mesh_enabled: bool,
    /// Whether strict mTLS is enforced for mesh traffic.
    pub mesh_strict_mtls: bool,
    /// Platform API URL (for OTLP endpoint, service discovery).
    pub platform_api_url: String,
    /// Gateway name for `HTTPRoute` parent refs.
    pub gateway_name: String,
    /// Master key for secret decryption (hex-encoded AES-256).
    pub master_key: Option<String>,
}

impl DeployerConfig {
    /// Compute the K8s namespace for a project environment.
    pub fn project_namespace(&self, slug: &str, env_suffix: &str) -> String {
        match &self.ns_prefix {
            Some(prefix) => format!("{prefix}-{slug}-{env_suffix}"),
            None => format!("{slug}-{env_suffix}"),
        }
    }
}

/// Shared state for the deployer subsystem.
///
/// Generic over `Svc: ReconcilerServices` so the binary can plug in concrete
/// implementations while tests use mocks.
#[derive(Clone)]
pub struct DeployerState<Svc: ReconcilerServices> {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub kube: kube::Client,
    pub minio: opendal::Operator,
    pub config: DeployerConfig,
    pub deploy_notify: Arc<tokio::sync::Notify>,
    pub task_heartbeat: Arc<dyn TaskHeartbeat>,
    pub services: Svc,
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// No-op `ReconcilerServices` for tests.
#[derive(Clone, Default)]
pub struct MockReconcilerServices;

impl ReconcilerServices for MockReconcilerServices {
    async fn fire_webhooks(&self, _: Uuid, _: &str, _: &serde_json::Value) {}

    async fn render_and_apply(
        &self,
        _: &kube::Client,
        _: &str,
        _: &serde_json::Value,
        _: &str,
        _: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn ops_read_file(&self, _: &Path, _: &str, _: &str) -> Option<String> {
        None
    }

    async fn ops_read_dir_yaml(&self, _: &Path, _: &str, _: &str) -> Option<String> {
        None
    }

    async fn ops_commit_values(
        &self,
        _: &Path,
        _: &str,
        _: &[(&str, &str)],
        _: &str,
    ) -> anyhow::Result<String> {
        Ok("mock-sha".to_string())
    }

    async fn decrypt_secret(&self, _: Uuid, _: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn ensure_namespace(
        &self,
        _: &kube::Client,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete_namespace(&self, _: &kube::Client, _: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn sync_ops_repo(
        &self,
        _: &PgPool,
        _: Uuid,
    ) -> anyhow::Result<(PathBuf, String, String)> {
        Ok((PathBuf::from("/tmp/mock"), "mock-sha".into(), "main".into()))
    }

    async fn get_branch_sha(&self, _: &Path, _: &str) -> anyhow::Result<String> {
        Ok("mock-sha".into())
    }

    async fn get_head_sha(&self, _: &Path) -> anyhow::Result<String> {
        Ok("mock-sha".into())
    }

    async fn read_values(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    fn generate_api_token(&self) -> (String, String) {
        ("mock-token".into(), "mock-hash".into())
    }

    async fn publish_event(&self, _: &fred::clients::Pool, _: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn check_condition(&self, _: &str, _: Option<f64>, _: Option<f64>) -> bool {
        false
    }

    async fn evaluate_metric(
        &self,
        _: &PgPool,
        _: &str,
        _: Option<&serde_json::Value>,
        _: &str,
        _: i32,
    ) -> anyhow::Result<Option<f64>> {
        Ok(None)
    }
}
