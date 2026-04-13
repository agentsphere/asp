// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Domain types for git operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A git user identity resolved from authentication (basic auth, SSH key, API token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitUser {
    pub user_id: Uuid,
    pub user_name: String,
    pub ip_addr: Option<String>,
    pub boundary_project_id: Option<Uuid>,
    pub boundary_workspace_id: Option<Uuid>,
    pub token_scopes: Option<Vec<String>>,
}

/// A project resolved from owner/repo path segments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedProject {
    pub project_id: Uuid,
    pub owner_id: Uuid,
    pub repo_disk_path: PathBuf,
    pub default_branch: String,
    pub visibility: String,
}

/// Author information for a git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub email: String,
}

/// Event emitted when a branch is pushed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushEvent {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub repo_path: PathBuf,
    pub branch: String,
    pub commit_sha: Option<String>,
}

/// Event emitted when a tag is pushed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEvent {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub repo_path: PathBuf,
    pub tag_name: String,
    pub commit_sha: Option<String>,
}

/// Event emitted when an MR-related branch is pushed (for MR sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrSyncEvent {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub repo_path: PathBuf,
    pub branch: String,
    pub commit_sha: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_user_debug() {
        let user = GitUser {
            user_id: Uuid::nil(),
            user_name: "alice".into(),
            ip_addr: Some("127.0.0.1".into()),
            boundary_project_id: None,
            boundary_workspace_id: None,
            token_scopes: Some(vec!["project:read".into()]),
        };
        let debug = format!("{user:?}");
        assert!(debug.contains("alice"));
    }

    #[test]
    fn resolved_project_debug() {
        let project = ResolvedProject {
            project_id: Uuid::nil(),
            owner_id: Uuid::nil(),
            repo_disk_path: PathBuf::from("/repos/test.git"),
            default_branch: "main".into(),
            visibility: "private".into(),
        };
        let debug = format!("{project:?}");
        assert!(debug.contains("main"));
    }

    #[test]
    fn push_event_serde_roundtrip() {
        let event = PushEvent {
            project_id: Uuid::nil(),
            user_id: Uuid::nil(),
            user_name: "alice".into(),
            repo_path: PathBuf::from("/repos/test.git"),
            branch: "main".into(),
            commit_sha: Some("abc123".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: PushEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.commit_sha.as_deref(), Some("abc123"));
    }

    #[test]
    fn tag_event_serde_roundtrip() {
        let event = TagEvent {
            project_id: Uuid::nil(),
            user_id: Uuid::nil(),
            repo_path: PathBuf::from("/repos/test.git"),
            tag_name: "v1.0.0".into(),
            commit_sha: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: TagEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tag_name, "v1.0.0");
    }

    #[test]
    fn mr_sync_event_serde_roundtrip() {
        let event = MrSyncEvent {
            project_id: Uuid::nil(),
            user_id: Uuid::nil(),
            repo_path: PathBuf::from("/repos/test.git"),
            branch: "feature/login".into(),
            commit_sha: Some("def456".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: MrSyncEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.branch, "feature/login");
    }
}
