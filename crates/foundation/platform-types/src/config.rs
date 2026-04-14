// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Composable configuration sub-structs.
//!
//! Each domain crate receives only the config slice it needs, rather than
//! depending on the monolith `Config`. The binary's `PlatformConfig` composes
//! all of these.

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

/// Server core configuration.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Listen address (e.g. `"0.0.0.0:8080"`).
    pub listen: String,
    /// Dev mode flag.
    pub dev_mode: bool,
    /// K8s namespace where the platform itself runs.
    pub platform_namespace: String,
    /// Optional namespace prefix for test isolation.
    pub ns_prefix: Option<String>,
    /// Global HTTP request timeout in seconds (default 300).
    pub request_timeout_secs: u64,
}

impl CoreConfig {
    /// Derive a project's K8s namespace: `{ns_prefix}-{slug}-{env}` or `{slug}-{env}`.
    pub fn project_namespace(&self, slug: &str, env: &str) -> String {
        match &self.ns_prefix {
            Some(prefix) => format!("{prefix}-{slug}-{env}"),
            None => format!("{slug}-{env}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// `PostgreSQL` connection configuration.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub database_url: String,
    /// Maximum connections (default 20).
    pub db_max_connections: u32,
    /// Connection acquire timeout in seconds (default 10).
    pub db_acquire_timeout_secs: u64,
}

// ---------------------------------------------------------------------------
// Valkey (Redis-compatible)
// ---------------------------------------------------------------------------

/// Valkey connection configuration.
#[derive(Debug, Clone)]
pub struct ValkeyConfig {
    pub valkey_url: String,
    /// Connection pool size (default 6).
    pub valkey_pool_size: usize,
    /// Host:port as seen from inside agent pods.
    pub valkey_agent_host: String,
}

// ---------------------------------------------------------------------------
// Object storage (MinIO / S3)
// ---------------------------------------------------------------------------

/// S3-compatible object storage configuration.
#[derive(Clone)]
pub struct StorageConfig {
    pub minio_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,
    /// Accept self-signed TLS certificates (dev/test only).
    pub minio_insecure: bool,
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageConfig")
            .field("minio_endpoint", &self.minio_endpoint)
            .field("minio_access_key", &"[REDACTED]")
            .field("minio_secret_key", &"[REDACTED]")
            .field("minio_insecure", &self.minio_insecure)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

/// Authentication and session configuration.
#[derive(Clone)]
pub struct AuthConfig {
    pub secure_cookies: bool,
    pub cors_origins: Vec<String>,
    pub trust_proxy_headers: bool,
    /// Trusted proxy CIDRs for X-Forwarded-For parsing.
    pub trust_proxy_cidrs: Vec<String>,
    /// Permission cache TTL in seconds (default 300).
    pub permission_cache_ttl_secs: u64,
    /// Maximum API token expiry in days (default 365).
    pub token_max_expiry_days: u32,
    /// Admin password (dev mode only).
    pub admin_password: Option<String>,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("secure_cookies", &self.secure_cookies)
            .field("cors_origins", &self.cors_origins)
            .field("trust_proxy_headers", &self.trust_proxy_headers)
            .field("trust_proxy_cidrs", &self.trust_proxy_cidrs)
            .field("permission_cache_ttl_secs", &self.permission_cache_ttl_secs)
            .field("token_max_expiry_days", &self.token_max_expiry_days)
            .field(
                "admin_password",
                &self.admin_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// WebAuthn
// ---------------------------------------------------------------------------

/// `WebAuthn` / Passkey configuration.
#[derive(Debug, Clone)]
pub struct WebAuthnConfig {
    /// Relying Party ID (domain, no protocol).
    pub webauthn_rp_id: String,
    /// Relying Party Origin (full URL).
    pub webauthn_rp_origin: String,
    /// Relying Party display name.
    pub webauthn_rp_name: String,
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

/// Git server configuration.
#[derive(Debug, Clone)]
pub struct GitConfig {
    pub git_repos_path: PathBuf,
    /// SSH listen address (e.g. `"0.0.0.0:2222"`). `None` disables SSH.
    pub ssh_listen: Option<String>,
    /// Path to ED25519 host key.
    pub ssh_host_key_path: String,
    /// Git smart HTTP operation timeout in seconds (default 600).
    pub git_http_timeout_secs: u64,
    /// Maximum LFS object size in bytes (default 5 GB).
    pub max_lfs_object_bytes: u64,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Pipeline executor configuration (mirrors `platform-pipeline::PipelineConfig`).
#[derive(Debug, Clone)]
pub struct PipelineSubConfig {
    /// Legacy fallback namespace for pipeline pods.
    pub pipeline_namespace: String,
    /// Max parallel steps in DAG execution.
    pub pipeline_max_parallel: usize,
    /// Pipeline timeout in seconds (default 3600).
    pub pipeline_timeout_secs: u64,
    /// Default runner image.
    pub runner_image: String,
    /// Git clone init container image.
    pub git_clone_image: String,
    /// Kaniko image for container builds.
    pub kaniko_image: String,
    /// URL for pods to reach the platform API.
    pub platform_api_url: String,
    /// Max single artifact file size in bytes.
    pub max_artifact_file_bytes: u64,
    /// Max total artifact size per pipeline in bytes.
    pub max_artifact_total_bytes: u64,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// Agent orchestration configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Legacy fallback namespace for agent pods.
    pub agent_namespace: String,
    /// Max concurrent CLI subprocesses per pod.
    pub max_cli_subprocesses: usize,
    /// Directory containing cross-compiled agent-runner binaries.
    pub agent_runner_dir: PathBuf,
    /// Directory containing cross-compiled platform-proxy binaries.
    pub proxy_binary_dir: PathBuf,
    /// Path to the MCP servers tarball.
    pub mcp_servers_tarball: PathBuf,
    /// Directory containing MCP server scripts.
    pub mcp_servers_path: String,
    /// Claude CLI version for auto-setup.
    pub claude_cli_version: String,
    /// Whether to spawn real CLI subprocesses.
    pub cli_spawn_enabled: bool,
    /// Idle timeout for agent sessions in seconds (default 1800).
    pub session_idle_timeout_secs: u64,
    /// Max running manager sessions per user (default 10).
    pub manager_session_max_per_user: i64,
}

// ---------------------------------------------------------------------------
// Deployer
// ---------------------------------------------------------------------------

/// Deployer / reconciler configuration.
#[derive(Debug, Clone)]
pub struct DeployerConfig {
    /// Ops repo storage path.
    pub ops_repos_path: PathBuf,
    /// External URL for preview proxy (dev only).
    pub preview_proxy_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Observe
// ---------------------------------------------------------------------------

/// Observability configuration.
#[derive(Debug, Clone)]
pub struct ObserveConfig {
    /// Data retention in days (default 30).
    pub observe_retention_days: u32,
    /// Ingest buffer capacity per signal type (default 10,000).
    pub observe_buffer_capacity: usize,
    /// Minimum tracing level for self-observability (default "warn").
    pub self_observe_level: String,
    /// Maximum alert rule window in seconds (default 86400 = 24h).
    pub alert_max_window_secs: u32,
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

/// Secrets engine configuration.
#[derive(Clone)]
pub struct SecretsConfig {
    /// AES-256-GCM master key (64-char hex).
    pub master_key: Option<String>,
    /// Previous master key for key rotation.
    pub master_key_previous: Option<String>,
}

impl std::fmt::Debug for SecretsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsConfig")
            .field(
                "master_key",
                &self.master_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "master_key_previous",
                &self.master_key_previous.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SMTP
// ---------------------------------------------------------------------------

/// Email (SMTP) configuration.
#[derive(Clone)]
pub struct SmtpConfig {
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_from: String,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_from", &self.smtp_from)
            .field("smtp_username", &self.smtp_username)
            .field(
                "smtp_password",
                &self.smtp_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// OCI registry configuration.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Platform's built-in OCI registry URL.
    pub registry_url: Option<String>,
    /// Node-accessible registry URL (via `DaemonSet` proxy).
    pub registry_node_url: Option<String>,
    /// Stream blobs through the platform instead of redirecting to `MinIO`.
    pub registry_proxy_blobs: bool,
    /// Directory containing OCI layout tarballs to seed.
    pub seed_images_path: PathBuf,
    /// Maximum HTTP body size for blob uploads in bytes (default 2 GB).
    pub registry_http_body_limit_bytes: usize,
    /// Maximum individual blob size in bytes (default 5 GB).
    pub registry_max_blob_size_bytes: u64,
}

// ---------------------------------------------------------------------------
// Service Mesh
// ---------------------------------------------------------------------------

/// Service mesh / mTLS configuration.
#[derive(Debug, Clone)]
pub struct MeshConfig {
    /// Enable the mesh CA module.
    pub mesh_enabled: bool,
    /// Enable strict mTLS mode.
    pub mesh_strict_mtls: bool,
    /// Leaf certificate TTL in seconds (default 3600).
    pub mesh_ca_cert_ttl_secs: u64,
    /// Root CA certificate validity in days (default 365).
    pub mesh_ca_root_ttl_days: u32,
    /// Path to prebuilt platform-proxy binary (dev/test only).
    pub proxy_binary_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

/// Gateway / ingress configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Name of the shared Gateway resource (default "platform-gateway").
    pub gateway_name: String,
    /// Namespace where the shared Gateway lives.
    pub gateway_namespace: String,
    /// Whether to auto-deploy the gateway DaemonSet/Deployment.
    pub gateway_auto_deploy: bool,
    /// Gateway HTTP listen port inside the pod.
    pub gateway_http_port: u16,
    /// Gateway TLS listen port inside the pod.
    pub gateway_tls_port: u16,
    /// `NodePort` for gateway HTTP (0 = auto-assign).
    pub gateway_http_node_port: u16,
    /// `NodePort` for gateway TLS (0 = auto-assign).
    pub gateway_tls_node_port: u16,
    /// Namespaces the gateway should watch. Empty = all labeled.
    pub gateway_watch_namespaces: Vec<String>,
    /// Enable ACME automatic certificate provisioning.
    pub acme_enabled: bool,
    /// ACME directory URL.
    pub acme_directory_url: String,
    /// ACME contact email.
    pub acme_contact_email: Option<String>,
}

// ---------------------------------------------------------------------------
// Operator
// ---------------------------------------------------------------------------

/// Platform operator / lifecycle configuration.
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// Health check interval in seconds (default 15).
    pub health_check_interval_secs: u64,
    /// Directory containing `.md` command templates to seed.
    pub seed_commands_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Webhook
// ---------------------------------------------------------------------------

/// Webhook delivery configuration.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Maximum concurrent webhook deliveries (default 50).
    pub webhook_max_concurrent: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_project_namespace_no_prefix() {
        let config = CoreConfig {
            listen: "0.0.0.0:8080".into(),
            dev_mode: false,
            platform_namespace: "platform".into(),
            ns_prefix: None,
            request_timeout_secs: 300,
        };
        assert_eq!(config.project_namespace("myapp", "dev"), "myapp-dev");
    }

    #[test]
    fn core_project_namespace_with_prefix() {
        let config = CoreConfig {
            listen: "0.0.0.0:8080".into(),
            dev_mode: false,
            platform_namespace: "platform".into(),
            ns_prefix: Some("test".into()),
            request_timeout_secs: 300,
        };
        assert_eq!(config.project_namespace("myapp", "dev"), "test-myapp-dev");
    }

    #[test]
    fn storage_config_debug_redacts_secrets() {
        let config = StorageConfig {
            minio_endpoint: "http://localhost:9000".into(),
            minio_access_key: "secret-key".into(),
            minio_secret_key: "super-secret".into(),
            minio_insecure: false,
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret-key"));
        assert!(!debug.contains("super-secret"));
    }

    #[test]
    fn secrets_config_debug_redacts() {
        let config = SecretsConfig {
            master_key: Some("0123456789abcdef".repeat(4)),
            master_key_previous: None,
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("0123456789abcdef"));
    }

    #[test]
    fn smtp_config_debug_redacts_password() {
        let config = SmtpConfig {
            smtp_host: Some("mail.example.com".into()),
            smtp_port: 587,
            smtp_from: "noreply@example.com".into(),
            smtp_username: Some("user".into()),
            smtp_password: Some("hunter2".into()),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("hunter2"));
    }

    #[test]
    fn auth_config_debug_redacts_admin_password() {
        let config = AuthConfig {
            secure_cookies: true,
            cors_origins: vec![],
            trust_proxy_headers: false,
            trust_proxy_cidrs: vec![],
            permission_cache_ttl_secs: 300,
            token_max_expiry_days: 365,
            admin_password: Some("admin123".into()),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("admin123"));
    }
}
