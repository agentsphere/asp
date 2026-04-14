// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! OCI container registry: types, digest, access control, seeding, pull secrets, and GC.
//!
//! This crate contains the pure logic and DB logic for the platform's built-in
//! OCI Distribution Spec v2 registry. HTTP handlers stay in `src/registry/` and
//! call into crate functions.

pub mod access;
pub mod credential_provider;
pub mod digest;
pub mod error;
pub mod gc;
pub mod pull_secret;
pub mod seed;
pub mod state;
pub mod types;

// Re-export key types at crate root.
pub use access::{RegistryUser, RepoAccess, copy_tag, glob_match, matches_tag_pattern};
pub use credential_provider::RegistryCredentials;
pub use digest::{Digest, sha256_digest};
pub use error::{OciErrorCode, RegistryError};
pub use gc::collect_garbage;
pub use pull_secret::{
    PullSecretResult, PushSecretResult, build_docker_config, cleanup_pull_secret,
    create_pull_secret, create_push_secret,
};
pub use seed::{SeedResult, seed_all, seed_image};
pub use state::{RegistryConfig, RegistryState};
pub use types::{
    Descriptor, MEDIA_TYPE_DOCKER_MANIFEST, MEDIA_TYPE_DOCKER_MANIFEST_LIST, MEDIA_TYPE_OCI_INDEX,
    MEDIA_TYPE_OCI_MANIFEST, OciManifest, TagListResponse, UploadSession, is_manifest_media_type,
};
