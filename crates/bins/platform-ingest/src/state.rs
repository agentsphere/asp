// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

/// Shared state for the ingest binary.
#[derive(Clone)]
pub struct IngestState {
    pub pool: sqlx::PgPool,
    pub valkey: fred::clients::Pool,
    pub trust_proxy: bool,
}
