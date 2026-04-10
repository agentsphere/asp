// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[tracing::instrument(skip(url), err)]
pub async fn connect(
    url: &str,
    max_connections: u32,
    acquire_timeout_secs: u64,
) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800)) // 30 min — recycle stale conns
        .connect(url)
        .await?;

    tracing::info!("connected to postgres");

    sqlx::migrate!().run(&pool).await?;
    tracing::info!("migrations applied");

    Ok(pool)
}
