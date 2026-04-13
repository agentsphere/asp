// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Trait definitions for git operations.
//!
//! Core traits (`GitCoreRead`, `GitWriter`, `GitMerger`) are defined in
//! `platform-types` and re-exported here. This module extends them with
//! browser-specific traits (`GitRepo`) and app-specific traits that require
//! DB access.
//!
//! Traits with default CLI implementations in this crate:
//! - [`GitRepo`], [`GitRepoManager`]
//!
//! Traits without default implementations (require DB access):
//! - [`PostReceiveHandler`], [`BranchProtectionProvider`], [`GitAuthenticator`],
//!   [`GitAccessControl`], [`ProjectResolver`]

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::browser_types::{BlobContent, BranchInfo, CommitInfo, TreeEntry};
use crate::error::GitError;
use crate::protection::BranchProtection;
use crate::templates::TemplateFile;
use crate::types::{GitUser, MrSyncEvent, PushEvent, ResolvedProject, TagEvent};

// Re-export core traits from platform-types.
pub use platform_types::{GitCoreRead, GitMerger, GitWriter};

// ---------------------------------------------------------------------------
// 1. GitRepo — Browser/UI read operations on bare repos
// ---------------------------------------------------------------------------

/// Extended read-only operations on a git repository.
///
/// Inherits core read methods from [`GitCoreRead`]. Adds browser/UI-specific
/// methods (tree listing with metadata, blob content, branch listing,
/// commit log, commit detail).
pub trait GitRepo: GitCoreRead {
    /// List tree entries at a given ref/path via `git ls-tree`.
    fn list_tree(
        &self,
        repo: &Path,
        git_ref: &str,
        path: Option<&str>,
    ) -> impl Future<Output = Result<Vec<TreeEntry>, GitError>> + Send;

    /// Read a blob at a given ref/path, up to `max_bytes`.
    fn show_blob(
        &self,
        repo: &Path,
        git_ref: &str,
        path: &str,
        max_bytes: usize,
    ) -> impl Future<Output = Result<BlobContent, GitError>> + Send;

    /// List all branches in the repo.
    fn list_branches(
        &self,
        repo: &Path,
    ) -> impl Future<Output = Result<Vec<BranchInfo>, GitError>> + Send;

    /// List commits on a ref with pagination.
    fn log_commits(
        &self,
        repo: &Path,
        git_ref: &str,
        limit: usize,
        offset: usize,
    ) -> impl Future<Output = Result<Vec<CommitInfo>, GitError>> + Send;

    /// Get details for a single commit by SHA.
    fn commit_detail(
        &self,
        repo: &Path,
        sha: &str,
    ) -> impl Future<Output = Result<CommitInfo, GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 2. GitRepoManager — Create/init repos, tags
// ---------------------------------------------------------------------------

/// Repository lifecycle operations.
pub trait GitRepoManager: Send + Sync {
    /// Initialize a bare repo with default template files.
    fn init_bare(
        &self,
        repos_path: &Path,
        owner: &str,
        name: &str,
        default_branch: &str,
    ) -> impl Future<Output = Result<PathBuf, GitError>> + Send;

    /// Initialize a bare repo with custom files.
    fn init_bare_with_files(
        &self,
        repos_path: &Path,
        owner: &str,
        name: &str,
        default_branch: &str,
        files: &[TemplateFile],
    ) -> impl Future<Output = Result<PathBuf, GitError>> + Send;

    /// Create an annotated tag at a commit.
    fn create_annotated_tag(
        &self,
        repo: &Path,
        name: &str,
        sha: &str,
        message: &str,
    ) -> impl Future<Output = Result<(), GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 3. PostReceiveHandler — Side effects after push (app-specific)
// ---------------------------------------------------------------------------

/// Side effects after a git push (pipeline triggers, webhooks, MR sync).
/// No default implementation — the main binary provides an in-process impl,
/// and a standalone git binary would publish to Valkey instead.
pub trait PostReceiveHandler: Send + Sync {
    fn on_push(&self, params: &PushEvent)
    -> impl Future<Output = Result<(), anyhow::Error>> + Send;

    fn on_tag(&self, params: &TagEvent) -> impl Future<Output = Result<(), anyhow::Error>> + Send;

    fn on_mr_sync(
        &self,
        params: &MrSyncEvent,
    ) -> impl Future<Output = Result<(), anyhow::Error>> + Send;
}

// ---------------------------------------------------------------------------
// 4. BranchProtectionProvider — Protection rule lookup (DB-backed)
// ---------------------------------------------------------------------------

/// Look up branch protection rules. Requires DB access — no default impl.
pub trait BranchProtectionProvider: Send + Sync {
    fn get_protection(
        &self,
        project_id: Uuid,
        branch: &str,
    ) -> impl Future<Output = Result<Option<BranchProtection>, anyhow::Error>> + Send;
}

// ---------------------------------------------------------------------------
// 5. GitAuthenticator — Auth for git transports (DB-backed)
// ---------------------------------------------------------------------------

/// Authenticate users for git transport (HTTP basic auth, SSH keys).
/// Requires DB access — no default impl.
pub trait GitAuthenticator: Send + Sync {
    fn authenticate_basic(
        &self,
        username: &str,
        password: &str,
    ) -> impl Future<Output = Result<GitUser, GitError>> + Send;

    fn authenticate_ssh_key(
        &self,
        fingerprint: &str,
    ) -> impl Future<Output = Result<GitUser, GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 6. GitAccessControl — Permission checks (DB-backed)
// ---------------------------------------------------------------------------

/// Check read/write access to a project's git repository.
/// Requires DB + RBAC — no default impl.
pub trait GitAccessControl: Send + Sync {
    fn check_read(
        &self,
        user: &GitUser,
        project: &ResolvedProject,
    ) -> impl Future<Output = Result<(), GitError>> + Send;

    fn check_write(
        &self,
        user: &GitUser,
        project: &ResolvedProject,
    ) -> impl Future<Output = Result<(), GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 7. ProjectResolver — Resolve owner/repo to project (DB-backed)
// ---------------------------------------------------------------------------

/// Resolve owner/repo path segments to a project. Requires DB — no default impl.
pub trait ProjectResolver: Send + Sync {
    fn resolve(
        &self,
        owner: &str,
        repo: &str,
    ) -> impl Future<Output = Result<ResolvedProject, GitError>> + Send;
}

// Bring `Future` into scope for RPITIT.
use std::future::Future;
