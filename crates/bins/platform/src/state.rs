// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Platform state composing all domain substates.

use std::collections::HashMap;
use std::sync::Arc;

use platform_types::AuditLog;
use platform_types::health::TaskRegistry;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::PlatformConfig;

/// Central state for the platform binary.
///
/// Holds shared infrastructure resources and a reference to config.
/// Domain-specific substates are constructed from these shared resources
/// when needed by API handlers and background tasks.
#[allow(dead_code)]
#[derive(Clone)]
pub struct PlatformState {
    // -- Shared infrastructure --
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub minio: opendal::Operator,
    pub kube: kube::Client,
    pub config: Arc<PlatformConfig>,

    // -- Coordination signals --
    pub pipeline_notify: Arc<tokio::sync::Notify>,
    pub deploy_notify: Arc<tokio::sync::Notify>,

    // -- WebAuthn --
    pub webauthn: Arc<webauthn_rs::prelude::Webauthn>,

    // -- Background task tracking --
    pub task_registry: Arc<TaskRegistry>,

    // -- Audit --
    pub audit_tx: AuditLog,

    // -- Concurrency control --
    pub webhook_semaphore: Arc<tokio::sync::Semaphore>,

    // -- Mesh CA (optional) --
    pub mesh_ca: Option<Arc<platform_mesh::MeshCa>>,

    // -- Health --
    pub health: Arc<tokio::sync::RwLock<platform_operator::health::HealthSnapshot>>,

    // -- Agent session state --
    pub secret_requests: Arc<tokio::sync::RwLock<HashMap<Uuid, serde_json::Value>>>,
    pub cli_sessions: Arc<tokio::sync::RwLock<HashMap<Uuid, serde_json::Value>>>,
}
