// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Error types for git operations.

use std::time::Duration;

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

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Error type for SSH public key parsing.
#[derive(Debug, thiserror::Error)]
pub enum SshKeyError {
    #[error("invalid SSH public key format")]
    InvalidFormat,

    #[error("unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("RSA key too short: {0} bits (minimum 2048)")]
    RsaKeyTooShort(u32),

    #[error("failed to compute fingerprint")]
    FingerprintError,
}

/// Error type for GPG public key parsing.
#[derive(Debug, thiserror::Error)]
pub enum GpgKeyError {
    #[error("invalid PGP public key armor")]
    InvalidArmor,

    #[error("failed to extract key metadata")]
    MetadataError,
}

/// Error type for SSH command parsing.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("invalid command")]
    InvalidCommand,

    #[error("dangerous path rejected")]
    PathTraversal,

    #[error("unsupported service: {0}")]
    UnsupportedService(String),
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
    }

    #[test]
    fn ssh_key_error_display() {
        assert_eq!(
            SshKeyError::InvalidFormat.to_string(),
            "invalid SSH public key format"
        );
        assert_eq!(
            SshKeyError::UnsupportedAlgorithm("dsa".into()).to_string(),
            "unsupported algorithm: dsa"
        );
        assert_eq!(
            SshKeyError::RsaKeyTooShort(1024).to_string(),
            "RSA key too short: 1024 bits (minimum 2048)"
        );
    }

    #[test]
    fn gpg_key_error_display() {
        assert_eq!(
            GpgKeyError::InvalidArmor.to_string(),
            "invalid PGP public key armor"
        );
        assert_eq!(
            GpgKeyError::MetadataError.to_string(),
            "failed to extract key metadata"
        );
    }

    #[test]
    fn ssh_error_display() {
        assert_eq!(SshError::InvalidCommand.to_string(), "invalid command");
        assert_eq!(
            SshError::PathTraversal.to_string(),
            "dangerous path rejected"
        );
        assert_eq!(
            SshError::UnsupportedService("git-archive".into()).to_string(),
            "unsupported service: git-archive"
        );
    }
}
