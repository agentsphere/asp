// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Focused state for the observe subsystem — no dependency on main binary's `AppState`.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::alert::AlertRouter;

/// Shared state for the observe subsystem.
#[derive(Clone)]
pub struct ObserveState {
    pub pool: PgPool,
    pub valkey: fred::clients::Pool,
    pub minio: opendal::Operator,
    pub config: ObserveConfig,
    pub alert_router: Arc<RwLock<AlertRouter>>,
}

/// Configuration for the observe subsystem.
#[derive(Clone)]
pub struct ObserveConfig {
    /// How many days of data to retain before purging.
    pub retention_days: u32,
    /// Channel buffer capacity per signal type.
    pub buffer_capacity: usize,
    /// Hours of log data to keep in Postgres before rotating to Parquet.
    pub parquet_log_retention_hours: u32,
    /// Hours of metric data to keep in Postgres before rotating to Parquet.
    pub parquet_metric_retention_hours: u32,
    /// Whether to trust X-Forwarded-For for client IP extraction.
    pub trust_proxy: bool,
    /// Maximum alert rule window in seconds (default 86400 = 24h).
    /// Controls how long the stream evaluator keeps samples in memory.
    pub alert_max_window_secs: u32,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            retention_days: 30,
            buffer_capacity: 10_000,
            parquet_log_retention_hours: 48,
            parquet_metric_retention_hours: 1,
            trust_proxy: false,
            alert_max_window_secs: 86_400,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_sensible_values() {
        let cfg = ObserveConfig::default();
        assert_eq!(cfg.retention_days, 30);
        assert_eq!(cfg.buffer_capacity, 10_000);
        assert_eq!(cfg.parquet_log_retention_hours, 48);
        assert_eq!(cfg.parquet_metric_retention_hours, 1);
        assert!(!cfg.trust_proxy);
        assert_eq!(cfg.alert_max_window_secs, 86_400);
    }
}
