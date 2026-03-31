// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use crate::error::ApiError;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // NotFound, StepFailed used in executor error paths
pub enum PipelineError {
    #[error("invalid pipeline definition: {0}")]
    InvalidDefinition(String),

    #[error("pipeline not found")]
    NotFound,

    #[error("step failed: {name} (exit code {exit_code})")]
    StepFailed { name: String, exit_code: i32 },

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Kube(#[from] kube::Error),

    #[error(transparent)]
    Storage(#[from] opendal::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<PipelineError> for ApiError {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::InvalidDefinition(msg) => Self::BadRequest(msg),
            PipelineError::NotFound => Self::NotFound("pipeline".into()),
            PipelineError::StepFailed { .. } => Self::Internal(err.into()),
            PipelineError::Db(e) => Self::from(e),
            PipelineError::Kube(e) => Self::from(e),
            PipelineError::Storage(e) => Self::from(e),
            PipelineError::Other(e) => Self::Internal(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_definition_maps_to_bad_request() {
        let api: ApiError = PipelineError::InvalidDefinition("bad yaml".into()).into();
        assert!(matches!(api, ApiError::BadRequest(msg) if msg == "bad yaml"));
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let api: ApiError = PipelineError::NotFound.into();
        assert!(matches!(api, ApiError::NotFound(msg) if msg == "pipeline"));
    }

    #[test]
    fn step_failed_maps_to_internal() {
        let api: ApiError = PipelineError::StepFailed {
            name: "build".into(),
            exit_code: 1,
        }
        .into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn other_maps_to_internal() {
        let api: ApiError = PipelineError::Other(anyhow::anyhow!("boom")).into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn display_step_failed() {
        let err = PipelineError::StepFailed {
            name: "build".into(),
            exit_code: 42,
        };
        let msg = err.to_string();
        assert!(msg.contains("build"));
        assert!(msg.contains("42"));
    }

    #[test]
    fn display_invalid_definition() {
        let err = PipelineError::InvalidDefinition("bad yaml".into());
        assert_eq!(err.to_string(), "invalid pipeline definition: bad yaml");
    }

    #[test]
    fn display_not_found() {
        let err = PipelineError::NotFound;
        assert_eq!(err.to_string(), "pipeline not found");
    }

    #[test]
    fn db_error_maps_to_api_error() {
        // Create a sqlx error via a known construction path
        let db_err = sqlx::Error::RowNotFound;
        let pipeline_err = PipelineError::Db(db_err);
        let api: ApiError = pipeline_err.into();
        // sqlx::Error::RowNotFound maps through ApiError::from(sqlx::Error)
        // which typically produces NotFound or Internal
        assert!(
            matches!(api, ApiError::NotFound(_) | ApiError::Internal(_)),
            "Db error should map to NotFound or Internal, got: {api:?}"
        );
    }

    #[test]
    fn from_opendal_creates_storage() {
        let opendal_err = opendal::Error::new(opendal::ErrorKind::Unexpected, "test");
        let pipeline_err: PipelineError = opendal_err.into();
        assert!(matches!(pipeline_err, PipelineError::Storage(_)));
    }

    #[test]
    fn other_error_maps_to_internal() {
        let err = PipelineError::Other(anyhow::anyhow!("something went wrong"));
        let msg = err.to_string();
        assert_eq!(msg, "something went wrong");
        let api: ApiError = err.into();
        assert!(matches!(api, ApiError::Internal(_)));
    }

    #[test]
    fn step_failed_error_fields() {
        let err = PipelineError::StepFailed {
            name: "deploy".into(),
            exit_code: 137,
        };
        // Verify the display includes name and exit code
        let display = format!("{err}");
        assert!(display.contains("deploy"));
        assert!(display.contains("137"));
    }

    #[test]
    fn from_anyhow_creates_other() {
        let anyhow_err = anyhow::anyhow!("test error");
        let pipeline_err: PipelineError = anyhow_err.into();
        assert!(matches!(pipeline_err, PipelineError::Other(_)));
    }

    #[test]
    fn storage_error_maps_to_internal() {
        let storage_err = opendal::Error::new(opendal::ErrorKind::NotFound, "blob not found");
        let pipeline_err = PipelineError::Storage(storage_err);
        let api: ApiError = pipeline_err.into();
        // opendal errors always map to Internal (see From<opendal::Error> for ApiError)
        assert!(
            matches!(api, ApiError::Internal(_)),
            "Storage error should map to Internal, got: {api:?}"
        );
    }

    #[test]
    fn from_sqlx_creates_db() {
        let sqlx_err = sqlx::Error::RowNotFound;
        let pipeline_err: PipelineError = sqlx_err.into();
        assert!(matches!(pipeline_err, PipelineError::Db(_)));
    }

    #[test]
    fn from_sqlx_row_not_found_maps_to_not_found() {
        let sqlx_err = sqlx::Error::RowNotFound;
        let pipeline_err = PipelineError::Db(sqlx_err);
        let api: ApiError = pipeline_err.into();
        // sqlx::Error::RowNotFound -> ApiError::NotFound via ApiError::from(sqlx::Error)
        assert!(
            matches!(api, ApiError::NotFound(_)),
            "RowNotFound should map to NotFound, got: {api:?}"
        );
    }
}
