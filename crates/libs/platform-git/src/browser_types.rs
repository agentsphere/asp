// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Types for the repository browser (tree, blob, branch, commit).

use serde::{Deserialize, Serialize};

use crate::signature::SignatureInfo;

/// An entry in a git tree (file or directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    /// "blob" or "tree"
    pub entry_type: String,
    /// e.g. "100644", "040000", "100755"
    pub mode: String,
    pub size: Option<i64>,
    pub sha: String,
}

/// Content of a git blob, with binary detection.
#[derive(Debug, Clone)]
pub struct BlobContent {
    pub content: Vec<u8>,
    pub size: i64,
    pub is_binary: bool,
}

/// Information about a git branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub sha: String,
    pub updated_at: String,
}

/// Information about a git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: String,
    pub committer_name: String,
    pub committer_email: String,
    pub committed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_entry_serialization() {
        let entry = TreeEntry {
            name: "README.md".into(),
            entry_type: "blob".into(),
            mode: "100644".into(),
            size: Some(1234),
            sha: "abc123".into(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["name"], "README.md");
        assert_eq!(json["entry_type"], "blob");
        assert_eq!(json["size"], 1234);
    }

    #[test]
    fn tree_entry_null_size() {
        let entry = TreeEntry {
            name: "src".into(),
            entry_type: "tree".into(),
            mode: "040000".into(),
            size: None,
            sha: "def456".into(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json["size"].is_null());
    }

    #[test]
    fn branch_info_serialization() {
        let info = BranchInfo {
            name: "feature/x".into(),
            sha: "abc".into(),
            updated_at: "2026-01-01".into(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["name"], "feature/x");
    }

    #[test]
    fn commit_info_no_signature() {
        let info = CommitInfo {
            sha: "abc".into(),
            message: "test".into(),
            author_name: "A".into(),
            author_email: "a@e".into(),
            authored_at: "d".into(),
            committer_name: "C".into(),
            committer_email: "c@e".into(),
            committed_at: "d".into(),
            signature: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("signature").is_none());
    }

    #[test]
    fn commit_info_with_signature() {
        use crate::signature::SignatureStatus;
        let info = CommitInfo {
            sha: "abc".into(),
            message: "test".into(),
            author_name: "A".into(),
            author_email: "a@e".into(),
            authored_at: "d".into(),
            committer_name: "C".into(),
            committer_email: "c@e".into(),
            committed_at: "d".into(),
            signature: Some(SignatureInfo {
                status: SignatureStatus::Verified,
                signer_key_id: Some("KEY123".into()),
                signer_fingerprint: Some("FP456".into()),
                signer_name: Some("Alice".into()),
            }),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("signature").is_some());
        assert_eq!(json["signature"]["signer_key_id"], "KEY123");
    }

    #[test]
    fn blob_content_debug() {
        let blob = BlobContent {
            content: b"hello".to_vec(),
            size: 5,
            is_binary: false,
        };
        let debug = format!("{blob:?}");
        assert!(debug.contains("BlobContent"));
    }
}
