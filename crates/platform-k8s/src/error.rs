// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

/// Errors from K8s namespace and resource operations.
#[derive(Debug, thiserror::Error)]
pub enum K8sError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error(transparent)]
    Kube(#[from] kube::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
