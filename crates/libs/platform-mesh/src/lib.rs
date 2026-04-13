// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Service mesh CA module.
//!
//! Provides a SPIFFE-based certificate authority for mTLS between services,
//! ACME HTTP-01 provisioning, and trust bundle sync.
//!
//! No `AppState` dependency — takes explicit parameters (`PgPool`, `kube::Client`, etc.).

pub mod acme;
pub mod ca;
pub mod config;
pub mod error;
pub mod identity;
pub mod sync;

pub use acme::{parse_acme_challenge_path, run_acme_manager};
pub use ca::{CertBundle, MeshCa};
pub use config::{AcmeConfig, MeshConfig};
pub use error::MeshError;
pub use identity::SpiffeId;
pub use sync::{sync_bundles_to_namespaces, sync_trust_bundles};
