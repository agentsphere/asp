// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Error type for git operations.

use std::time::Duration;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Primary error type for git CLI operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git {command} failed: {stderr}")]
    CommandFailed { command: String, stderr: String },

    #[error("git command timed out after {0:?}")]
    Timeout(Duration),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("ref not found: {0}")]
    RefNotFound(String),

    #[error("file not found at {git_ref}:{path}")]
    FileNotFound { git_ref: String, path: String },

    #[error("merge conflict: {0}")]
    MergeConflict(String),

    #[error("invalid ref: {0}")]
    InvalidRef(String),

    #[error("path traversal: {0}")]
    PathTraversal(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for GitError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound(_) | Self::RefNotFound(_) | Self::FileNotFound { .. } => {
                StatusCode::NOT_FOUND
            }
            Self::BadRequest(_) | Self::InvalidRef(_) | Self::PathTraversal(_) => {
                StatusCode::BAD_REQUEST
            }
            Self::MergeConflict(_) => StatusCode::CONFLICT,
            Self::CommandFailed { .. } | Self::Timeout(_) | Self::Io(_) | Self::Other(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_error_display() {
        let err = GitError::CommandFailed {
            command: "merge".into(),
            stderr: "conflict".into(),
        };
        assert_eq!(err.to_string(), "git merge failed: conflict");

        let err = GitError::Timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30s"));

        let err = GitError::RefNotFound("main".into());
        assert_eq!(err.to_string(), "ref not found: main");

        let err = GitError::FileNotFound {
            git_ref: "HEAD".into(),
            path: "README.md".into(),
        };
        assert_eq!(err.to_string(), "file not found at HEAD:README.md");

        let err = GitError::BadRequest("invalid service".into());
        assert_eq!(err.to_string(), "bad request: invalid service");
    }
}
