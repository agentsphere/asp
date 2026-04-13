// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Configuration for the K8s watcher binary.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "platform-k8s-watcher")]
pub struct WatcherConfig {
    /// OTLP ingest endpoint (e.g. `http://platform-ingest:8081`).
    #[arg(long, env = "INGEST_ENDPOINT")]
    pub ingest_endpoint: String,

    /// Bearer token for the ingest endpoint.
    #[arg(long, env = "INGEST_TOKEN")]
    pub ingest_token: String,

    /// Comma-separated namespaces to watch. Empty = cluster-wide.
    #[arg(long, env = "WATCH_NAMESPACES", default_value = "")]
    pub watch_namespaces: String,

    /// Metrics flush interval in seconds.
    #[arg(long, env = "FLUSH_INTERVAL_SECS", default_value = "30")]
    pub flush_interval_secs: u64,

    /// Health endpoint listen address.
    #[arg(long, env = "LISTEN", default_value = "0.0.0.0:8082")]
    pub listen: String,
}

impl WatcherConfig {
    /// Parse comma-separated namespace list. Empty string → empty vec (cluster-wide).
    pub fn namespaces(&self) -> Vec<String> {
        if self.watch_namespaces.is_empty() {
            Vec::new()
        } else {
            self.watch_namespaces
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_empty() {
        let cfg = WatcherConfig {
            ingest_endpoint: String::new(),
            ingest_token: String::new(),
            watch_namespaces: String::new(),
            flush_interval_secs: 30,
            listen: String::new(),
        };
        assert!(cfg.namespaces().is_empty());
    }

    #[test]
    fn namespaces_single() {
        let cfg = WatcherConfig {
            ingest_endpoint: String::new(),
            ingest_token: String::new(),
            watch_namespaces: "platform".into(),
            flush_interval_secs: 30,
            listen: String::new(),
        };
        assert_eq!(cfg.namespaces(), vec!["platform"]);
    }

    #[test]
    fn namespaces_multiple_with_whitespace() {
        let cfg = WatcherConfig {
            ingest_endpoint: String::new(),
            ingest_token: String::new(),
            watch_namespaces: " platform , default , kube-system ".into(),
            flush_interval_secs: 30,
            listen: String::new(),
        };
        assert_eq!(cfg.namespaces(), vec!["platform", "default", "kube-system"]);
    }

    #[test]
    fn namespaces_trailing_comma() {
        let cfg = WatcherConfig {
            ingest_endpoint: String::new(),
            ingest_token: String::new(),
            watch_namespaces: "ns1,ns2,".into(),
            flush_interval_secs: 30,
            listen: String::new(),
        };
        assert_eq!(cfg.namespaces(), vec!["ns1", "ns2"]);
    }
}
