// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Connect to Postgres WITHOUT running migrations.
/// The main binary runs `sqlx::migrate!()` separately; the ingest binary
/// expects an already-migrated database.
#[tracing::instrument(skip(url), err)]
pub async fn pg_connect(
    url: &str,
    max_connections: u32,
    acquire_timeout_secs: u64,
) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect(url)
        .await?;

    tracing::info!("connected to postgres");

    Ok(pool)
}
