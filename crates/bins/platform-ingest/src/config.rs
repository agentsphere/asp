// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "platform-ingest", about = "Standalone OTLP ingest service")]
pub struct IngestConfig {
    /// Postgres connection URL.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,

    /// Valkey connection URL.
    #[arg(long, env = "VALKEY_URL")]
    pub valkey_url: String,

    /// Listen address (e.g. "0.0.0.0:8081").
    #[arg(long, env = "LISTEN", default_value = "0.0.0.0:8081")]
    pub listen: String,

    /// Trust X-Forwarded-For header for client IP.
    #[arg(long, env = "TRUST_PROXY", default_value = "false")]
    pub trust_proxy: bool,

    /// Ingest buffer capacity per signal type.
    #[arg(long, env = "BUFFER_CAPACITY", default_value = "10000")]
    pub buffer_capacity: usize,

    /// Permission cache TTL in seconds.
    #[arg(long, env = "PERMISSION_CACHE_TTL", default_value = "300")]
    pub permission_cache_ttl: u64,
}
