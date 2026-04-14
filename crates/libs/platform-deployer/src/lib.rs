// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

pub mod analysis;
pub mod applier;
pub mod error;
pub mod gateway;
pub mod image_inspect;
pub mod preview;
pub mod reconciler;
pub mod renderer;
pub mod state;
pub mod types;

pub use error::DeployerError;
pub use state::{DeployerState, MockReconcilerServices, ReconcilerServices};

use platform_types::ManifestApplier;

/// Service struct that implements `ManifestApplier` from `platform-types`.
pub struct DeployerService;

impl ManifestApplier for DeployerService {
    async fn render_and_apply(
        &self,
        kube: &kube::Client,
        manifest: &str,
        vars: &serde_json::Value,
        namespace: &str,
        tracking: Option<&str>,
    ) -> anyhow::Result<()> {
        let rendered = renderer::render_from_value(manifest, vars)?;
        let tracking_id = tracking.and_then(|t| uuid::Uuid::parse_str(t).ok());
        applier::apply_with_tracking(kube, &rendered, namespace, tracking_id).await?;
        Ok(())
    }
}
