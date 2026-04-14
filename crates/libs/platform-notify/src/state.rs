// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Notify subsystem state — no dependency on the main binary's `AppState`.

use std::sync::Arc;

use sqlx::PgPool;

use crate::email::SmtpConfig;

/// Shared state for the notification subsystem.
#[derive(Clone)]
pub struct NotifyState {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub config: Arc<SmtpConfig>,
}
