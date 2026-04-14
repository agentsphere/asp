// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Mesh CA error types.

use platform_types::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("mesh CA not enabled")]
    NotEnabled,

    #[error("invalid SPIFFE identity: {0}")]
    InvalidSpiffeId(String),

    #[error("certificate generation failed: {0}")]
    CertGeneration(String),

    #[error("CA initialization failed: {0}")]
    CaInit(String),

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Secrets(#[from] anyhow::Error),
}

impl From<MeshError> for ApiError {
    fn from(err: MeshError) -> Self {
        match err {
            MeshError::NotEnabled => Self::ServiceUnavailable("mesh CA not enabled".into()),
            MeshError::InvalidSpiffeId(msg) => Self::BadRequest(msg),
            MeshError::CertGeneration(msg) => {
                tracing::error!(error = %msg, "mesh certificate generation failed");
                Self::Internal(anyhow::anyhow!(msg))
            }
            MeshError::CaInit(msg) => {
                tracing::error!(error = %msg, "mesh CA initialization failed");
                Self::Internal(anyhow::anyhow!(msg))
            }
            MeshError::Db(e) => Self::from(e),
            MeshError::Secrets(e) => Self::Internal(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Display tests --

    #[test]
    fn not_enabled_display() {
        assert_eq!(MeshError::NotEnabled.to_string(), "mesh CA not enabled");
    }

    #[test]
    fn invalid_spiffe_id_display() {
        let err = MeshError::InvalidSpiffeId("bad namespace".into());
        assert_eq!(err.to_string(), "invalid SPIFFE identity: bad namespace");
    }

    #[test]
    fn cert_generation_display() {
        let err = MeshError::CertGeneration("key too short".into());
        assert_eq!(
            err.to_string(),
            "certificate generation failed: key too short"
        );
    }

    #[test]
    fn ca_init_display() {
        let err = MeshError::CaInit("missing root cert".into());
        assert_eq!(
            err.to_string(),
            "CA initialization failed: missing root cert"
        );
    }

    // -- From conversion tests --

    #[test]
    fn from_sqlx_creates_db() {
        let sqlx_err = sqlx::Error::RowNotFound;
        let err: MeshError = sqlx_err.into();
        assert!(matches!(err, MeshError::Db(_)));
    }

    #[test]
    fn from_anyhow_creates_secrets() {
        let anyhow_err = anyhow::anyhow!("vault error");
        let err: MeshError = anyhow_err.into();
        assert!(matches!(err, MeshError::Secrets(_)));
    }

    // -- ApiError conversion tests --

    #[test]
    fn not_enabled_maps_to_service_unavailable() {
        let api: ApiError = MeshError::NotEnabled.into();
        assert!(matches!(api, ApiError::ServiceUnavailable(_)));
    }

    #[test]
    fn invalid_spiffe_id_maps_to_bad_request() {
        let api: ApiError = MeshError::InvalidSpiffeId("bad".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg == "bad"));
    }

    #[test]
    fn cert_generation_maps_to_internal() {
        let api: ApiError = MeshError::CertGeneration("oops".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn ca_init_maps_to_internal() {
        let api: ApiError = MeshError::CaInit("oops".into()).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn db_error_maps_to_api_error() {
        let api: ApiError = MeshError::Db(sqlx::Error::RowNotFound).into();
        assert!(matches!(api, ApiError::NotFound(_)));
    }

    #[test]
    fn secrets_error_maps_to_internal() {
        let api: ApiError = MeshError::Secrets(anyhow::anyhow!("vault down")).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }
}
