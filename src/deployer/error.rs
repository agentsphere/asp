// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use crate::error::ApiError;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum DeployerError {
    #[error("deployment not found")]
    NotFound,

    #[error("ops repo not found: {0}")]
    OpsRepoNotFound(String),

    #[error("ops repo sync failed: {0}")]
    SyncFailed(String),

    #[error("template render failed: {0}")]
    RenderFailed(String),

    #[error("manifest apply failed: {0}")]
    ApplyFailed(String),

    #[error("health check timed out after {0}s")]
    HealthTimeout(u64),

    #[error("no previous successful deployment for rollback")]
    NoPreviousDeployment,

    #[error("ops repo commit failed: {0}")]
    CommitFailed(String),

    #[error("ops repo revert failed: {0}")]
    RevertFailed(String),

    #[error("values file not found: {0}")]
    ValuesNotFound(String),

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("forbidden manifest: {0}")]
    ForbiddenManifest(String),

    #[error("invalid phase transition: {0} -> {1}")]
    InvalidTransition(String, String),

    #[error("gateway API error: {0}")]
    GatewayError(String),

    #[error("analysis failed: {0}")]
    AnalysisFailed(String),

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Kube(#[from] kube::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<DeployerError> for ApiError {
    fn from(err: DeployerError) -> Self {
        match err {
            DeployerError::NotFound | DeployerError::OpsRepoNotFound(_) => {
                Self::NotFound("deployment".into())
            }
            DeployerError::NoPreviousDeployment
            | DeployerError::RenderFailed(_)
            | DeployerError::InvalidManifest(_)
            | DeployerError::ForbiddenManifest(_)
            | DeployerError::ValuesNotFound(_)
            | DeployerError::InvalidTransition(_, _) => Self::BadRequest(err.to_string()),
            DeployerError::Db(e) => Self::from(e),
            DeployerError::Kube(e) => Self::from(e),
            DeployerError::Other(e) => Self::Internal(e),
            _ => Self::Internal(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_to_404() {
        let api: ApiError = DeployerError::NotFound.into();
        assert!(matches!(api, ApiError::NotFound(_)));
    }

    #[test]
    fn ops_repo_not_found_maps_to_404() {
        let api: ApiError = DeployerError::OpsRepoNotFound("test".into()).into();
        assert!(matches!(api, ApiError::NotFound(_)));
    }

    #[test]
    fn no_previous_deployment_maps_to_bad_request() {
        let api: ApiError = DeployerError::NoPreviousDeployment.into();
        assert!(matches!(api, ApiError::BadRequest(_)));
    }

    #[test]
    fn render_failed_maps_to_bad_request() {
        let api: ApiError = DeployerError::RenderFailed("bad template".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg.contains("bad template")));
    }

    #[test]
    fn invalid_manifest_maps_to_bad_request() {
        let api: ApiError = DeployerError::InvalidManifest("no kind".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg.contains("no kind")));
    }

    #[test]
    fn health_timeout_maps_to_internal() {
        let api: ApiError = DeployerError::HealthTimeout(300).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn sync_failed_maps_to_internal() {
        let api: ApiError = DeployerError::SyncFailed("git error".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn other_maps_to_internal() {
        let api: ApiError = DeployerError::Other(anyhow::anyhow!("boom")).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn invalid_transition_maps_to_bad_request() {
        let api: ApiError =
            DeployerError::InvalidTransition("pending".into(), "completed".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg.contains("pending")));
    }

    #[test]
    fn gateway_error_maps_to_internal() {
        let api: ApiError = DeployerError::GatewayError("no gateway".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn analysis_failed_maps_to_internal() {
        let api: ApiError = DeployerError::AnalysisFailed("metric missing".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn forbidden_manifest_maps_to_bad_request() {
        let api: ApiError =
            DeployerError::ForbiddenManifest("privileged not allowed".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg.contains("privileged not allowed")));
    }

    #[test]
    fn commit_failed_maps_to_internal() {
        let api: ApiError = DeployerError::CommitFailed("git error".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn revert_failed_maps_to_internal() {
        let api: ApiError = DeployerError::RevertFailed("revert error".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn values_not_found_maps_to_bad_request() {
        let api: ApiError = DeployerError::ValuesNotFound("staging.yaml".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg.contains("staging.yaml")));
    }

    #[test]
    fn apply_failed_maps_to_internal() {
        let api: ApiError = DeployerError::ApplyFailed("kube error".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    // -- Display trait tests --

    #[test]
    fn not_found_display() {
        let err = DeployerError::NotFound;
        assert_eq!(err.to_string(), "deployment not found");
    }

    #[test]
    fn ops_repo_not_found_display() {
        let err = DeployerError::OpsRepoNotFound("abc-123".into());
        assert_eq!(err.to_string(), "ops repo not found: abc-123");
    }

    #[test]
    fn sync_failed_display() {
        let err = DeployerError::SyncFailed("connection refused".into());
        assert_eq!(err.to_string(), "ops repo sync failed: connection refused");
    }

    #[test]
    fn render_failed_display() {
        let err = DeployerError::RenderFailed("missing variable".into());
        assert_eq!(err.to_string(), "template render failed: missing variable");
    }

    #[test]
    fn health_timeout_display() {
        let err = DeployerError::HealthTimeout(300);
        assert_eq!(err.to_string(), "health check timed out after 300s");
    }

    #[test]
    fn no_previous_deployment_display() {
        let err = DeployerError::NoPreviousDeployment;
        assert_eq!(
            err.to_string(),
            "no previous successful deployment for rollback"
        );
    }

    #[test]
    fn commit_failed_display() {
        let err = DeployerError::CommitFailed("git add failed".into());
        assert_eq!(err.to_string(), "ops repo commit failed: git add failed");
    }

    #[test]
    fn revert_failed_display() {
        let err = DeployerError::RevertFailed("merge conflict".into());
        assert_eq!(err.to_string(), "ops repo revert failed: merge conflict");
    }

    #[test]
    fn values_not_found_display() {
        let err = DeployerError::ValuesNotFound("staging.yaml at HEAD".into());
        assert_eq!(
            err.to_string(),
            "values file not found: staging.yaml at HEAD"
        );
    }

    #[test]
    fn invalid_manifest_display() {
        let err = DeployerError::InvalidManifest("missing kind".into());
        assert_eq!(err.to_string(), "invalid manifest: missing kind");
    }

    #[test]
    fn forbidden_manifest_display() {
        let err = DeployerError::ForbiddenManifest("hostNetwork not allowed".into());
        assert_eq!(
            err.to_string(),
            "forbidden manifest: hostNetwork not allowed"
        );
    }

    #[test]
    fn invalid_transition_display() {
        let err = DeployerError::InvalidTransition("completed".into(), "pending".into());
        assert_eq!(
            err.to_string(),
            "invalid phase transition: completed -> pending"
        );
    }

    #[test]
    fn gateway_error_display() {
        let err = DeployerError::GatewayError("weights sum to 110".into());
        assert_eq!(err.to_string(), "gateway API error: weights sum to 110");
    }

    #[test]
    fn analysis_failed_display() {
        let err = DeployerError::AnalysisFailed("metric query timeout".into());
        assert_eq!(err.to_string(), "analysis failed: metric query timeout");
    }

    #[test]
    fn apply_failed_display() {
        let err = DeployerError::ApplyFailed("server rejected".into());
        assert_eq!(err.to_string(), "manifest apply failed: server rejected");
    }
}
