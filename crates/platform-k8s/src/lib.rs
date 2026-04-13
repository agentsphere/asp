// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Shared K8s namespace lifecycle: naming, creation, RBAC, network policies, deletion.
//!
//! No `AppState` dependency — takes pre-extracted `kube::Client` and config values.

pub mod error;
pub mod namespace;

// Re-export key types at crate root.
pub use error::K8sError;
pub use namespace::{
    build_namespace_object, build_network_policy, build_session_network_policy, build_session_rbac,
    delete_namespace, ensure_mesh_ca_bundle, ensure_namespace, ensure_namespace_with_services_ns,
    ensure_network_policy, ensure_session_namespace, ensure_session_network_policy,
    pipeline_namespace_name, session_namespace_name, slugify_namespace, test_namespace_name,
};
