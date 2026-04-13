// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Secrets engine: AES-256-GCM encryption, user provider keys, LLM provider
//! configs, and CLI credential storage.
//!
//! All modules take `&PgPool` + `&[u8; 32]` (master key) as parameters.
//! No `AppState` dependency — designed for use from any binary.

pub mod cli_creds;
pub mod engine;
pub mod llm_providers;
pub mod user_keys;

// Re-export key types at crate root.
pub use engine::{
    CreateSecretParams, SecretMetadata, create_global_secret, create_secret, decrypt,
    delete_secret, dev_master_key, encrypt, list_secrets, list_workspace_secrets, parse_master_key,
    query_scoped_secrets, resolve_global_secret, resolve_secret, resolve_secret_hierarchical,
    resolve_secrets_for_env, validate_master_key,
};
