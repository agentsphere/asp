use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[tracing::instrument(skip(url), err)]
pub async fn connect(url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .connect(url)
        .await?;

    tracing::info!("connected to postgres");

    sqlx::migrate!().run(&pool).await?;
    tracing::info!("migrations applied");

    Ok(pool)
}
