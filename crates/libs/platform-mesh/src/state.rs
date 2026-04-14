// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Mesh subsystem state — no dependency on the main binary's `AppState`.

use std::sync::Arc;

use crate::ca::MeshCa;
use crate::config::MeshConfig;

/// Shared state for the service mesh subsystem.
#[derive(Clone)]
pub struct MeshState {
    pub kube: kube::Client,
    pub config: MeshConfig,
    pub mesh_ca: Option<Arc<MeshCa>>,
}
