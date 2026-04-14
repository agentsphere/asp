// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Error types for git operations.

// Core git error type — canonical definition in platform-types.
pub use platform_types::GitError;

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
        assert_eq!(
            SshKeyError::FingerprintError.to_string(),
            "failed to compute fingerprint"
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
