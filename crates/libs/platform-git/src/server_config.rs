// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Configuration for git server modules (smart HTTP, SSH, browser, LFS).

use std::path::PathBuf;

/// Configuration for git server operations.
///
/// Constructed by the host binary and passed into [`GitServerState`](crate::server_services::GitServerState).
#[derive(Debug, Clone)]
pub struct GitServerConfig {
    /// Root directory for git repositories.
    pub repos_path: PathBuf,

    /// Path to the SSH host key (ED25519). Generated if missing.
    pub ssh_host_key_path: Option<PathBuf>,

    /// SSH listen address (e.g. `"0.0.0.0:2222"`). If `None`, SSH server is disabled.
    pub ssh_listen_addr: Option<String>,

    /// Timeout in seconds for HTTP git operations (upload-pack, receive-pack).
    pub git_http_timeout_secs: u64,

    /// Maximum allowed LFS object size in bytes.
    pub max_lfs_object_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_debug() {
        let cfg = GitServerConfig {
            repos_path: PathBuf::from("/data/repos"),
            ssh_host_key_path: Some(PathBuf::from("/data/ssh_host_key")),
            ssh_listen_addr: Some("0.0.0.0:2222".into()),
            git_http_timeout_secs: 60,
            max_lfs_object_bytes: 500_000_000,
        };
        let debug = format!("{cfg:?}");
        assert!(debug.contains("GitServerConfig"));
        assert!(debug.contains("/data/repos"));
    }

    #[test]
    fn config_clone() {
        let cfg = GitServerConfig {
            repos_path: PathBuf::from("/repos"),
            ssh_host_key_path: None,
            ssh_listen_addr: None,
            git_http_timeout_secs: 30,
            max_lfs_object_bytes: 100_000_000,
        };
        let cloned = cfg.clone();
        assert_eq!(cloned.repos_path, cfg.repos_path);
        assert_eq!(cloned.git_http_timeout_secs, 30);
    }
}
