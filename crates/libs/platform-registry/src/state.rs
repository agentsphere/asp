// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Registry subsystem state — no dependency on the main binary's `AppState`.

use sqlx::PgPool;

/// Registry-specific configuration subset.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Platform's built-in OCI registry URL.
    pub registry_url: Option<String>,
    /// Node-accessible registry URL (via `DaemonSet` proxy).
    pub registry_node_url: Option<String>,
    /// Stream blobs through the platform instead of redirecting to `MinIO`.
    pub registry_proxy_blobs: bool,
    /// Maximum HTTP body size for blob uploads in bytes.
    pub registry_http_body_limit_bytes: usize,
    /// Maximum individual blob size in bytes.
    pub registry_max_blob_size_bytes: u64,
}

/// Shared state for the OCI registry subsystem.
#[derive(Clone)]
pub struct RegistryState {
    pub pool: PgPool,
    pub minio: opendal::Operator,
    pub kube: kube::Client,
    pub valkey: fred::clients::Pool,
    pub config: RegistryConfig,
}
