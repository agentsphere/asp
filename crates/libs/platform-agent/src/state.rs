// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Crate-local state for agent operations.

use std::pin::Pin;
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use crate::claude_cli::session::CliSessionManager;
use crate::config::AgentConfig;

/// Dyn-compatible webhook dispatcher for agent use.
///
/// The platform-types `WebhookDispatcher` uses `impl Future` which isn't
/// object-safe. This trait wraps it with boxed futures for `dyn` dispatch.
pub trait DynWebhookDispatcher: Send + Sync {
    fn fire_webhooks(
        &self,
        project_id: Uuid,
        event_name: &str,
        payload: &serde_json::Value,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}

/// Shared state for all agent operations.
///
/// Constructed from the main binary's `AppState` via `AppState::agent_state()`.
#[derive(Clone)]
pub struct AgentState {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub kube: kube::Client,
    pub minio: opendal::Operator,
    pub config: Arc<AgentConfig>,
    pub cli_sessions: CliSessionManager,
    pub task_registry: Arc<dyn platform_types::TaskHeartbeat>,
    pub webhook_dispatcher: Arc<dyn DynWebhookDispatcher>,
    pub webhook_semaphore: Arc<tokio::sync::Semaphore>,
}
