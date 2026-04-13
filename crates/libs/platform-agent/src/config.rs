// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Agent-specific configuration subset.

/// Configuration fields needed by the agent module.
///
/// Constructed from the main `Config` in the binary via `AgentConfig::new()`.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub platform_api_url: String,
    pub registry_url: Option<String>,
    pub registry_node_url: Option<String>,
    pub valkey_agent_host: String,
    pub claude_cli_version: String,
    pub runner_image: String,
    pub git_clone_image: String,
    pub agent_namespace: String,
    pub platform_namespace: String,
    pub gateway_namespace: String,
    pub ns_prefix: Option<String>,
    pub dev_mode: bool,
    pub session_idle_timeout_secs: u64,
    pub proxy_binary_path: Option<String>,
    pub master_key: Option<String>,
    pub listen: String,
    pub mcp_servers_path: String,
    pub manager_session_max_per_user: i64,
    pub cli_spawn_enabled: bool,
}
