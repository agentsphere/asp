// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Core git trait contracts for cross-crate use.
//!
//! These traits define the subset of git operations consumed by ops-repo,
//! deployer, and pipeline crates. Browser-specific methods (`show_blob`,
//! `log_commits`, etc.) remain in `platform-git`'s own `GitRepo` trait.

use std::future::Future;
use std::path::Path;

use crate::GitError;

// ---------------------------------------------------------------------------
// 1. GitCoreRead — Core read operations on bare repos
// ---------------------------------------------------------------------------

/// Core read-only operations on a git repository.
///
/// This is the minimal read interface consumed by ops-repo, deployer, and
/// pipeline. Browser-specific methods live in `platform-git::GitRepo`.
pub trait GitCoreRead: Send + Sync {
    /// Resolve a refspec to a commit SHA.
    fn rev_parse(
        &self,
        repo: &Path,
        refspec: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;

    /// Read a file at a given ref. Returns `None` if the file doesn't exist.
    fn read_file(
        &self,
        repo: &Path,
        git_ref: &str,
        path: &str,
    ) -> impl Future<Output = Result<Option<String>, GitError>> + Send;

    /// List directory entries at a given ref (one level, not recursive).
    fn list_dir(
        &self,
        repo: &Path,
        git_ref: &str,
        dir: &str,
    ) -> impl Future<Output = Result<Vec<String>, GitError>> + Send;

    /// List all file paths recursively under a directory at a given ref.
    fn list_tree_recursive(
        &self,
        repo: &Path,
        git_ref: &str,
        dir: &str,
    ) -> impl Future<Output = Result<Vec<String>, GitError>> + Send;

    /// Check if a branch exists in the repo.
    fn branch_exists(
        &self,
        repo: &Path,
        branch: &str,
    ) -> impl Future<Output = Result<bool, GitError>> + Send;

    /// Check if `potential_ancestor` is an ancestor of `commit`.
    fn is_ancestor(
        &self,
        repo: &Path,
        potential_ancestor: &str,
        commit: &str,
    ) -> impl Future<Output = Result<bool, GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 2. GitWriter — Write files via worktrees (locked)
// ---------------------------------------------------------------------------

/// Write files to a bare repo branch via worktrees.
/// Per-repo locking is handled by the implementation.
pub trait GitWriter: Send + Sync {
    /// Commit one or more files on a branch. Returns the new commit SHA.
    fn commit_files(
        &self,
        repo: &Path,
        branch: &str,
        files: &[(&str, &[u8])],
        message: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;

    /// Write files to a worktree, remove specified directories, then stage ALL
    /// changes (including deletions via `git add -A`) and commit.
    /// Used for sync operations where the working tree is fully replaced.
    /// Returns the new commit SHA.
    fn commit_all(
        &self,
        repo: &Path,
        branch: &str,
        files: &[(&str, &[u8])],
        remove_dirs: &[&str],
        message: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;

    /// Revert the last commit on a branch via `git revert HEAD --no-edit`.
    /// Returns the new commit SHA after the revert.
    fn revert_head(
        &self,
        repo: &Path,
        branch: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;
}

// ---------------------------------------------------------------------------
// 3. GitMerger — Merge strategies via worktrees
// ---------------------------------------------------------------------------

/// Merge operations on bare repos using worktrees.
pub trait GitMerger: Send + Sync {
    /// No-fast-forward merge of source into target.
    fn merge_no_ff(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
        message: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;

    /// Squash merge: all source commits into a single commit on target.
    fn squash_merge(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
        message: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;

    /// Rebase merge: fast-forward target to source (source must be rebased on target).
    fn rebase_merge(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
    ) -> impl Future<Output = Result<String, GitError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify traits compile with mock implementations (impl Future, not dyn).

    struct MockGitCoreRead;
    impl GitCoreRead for MockGitCoreRead {
        async fn rev_parse(&self, _repo: &Path, _refspec: &str) -> Result<String, GitError> {
            Ok("abc123".into())
        }
        async fn read_file(
            &self,
            _repo: &Path,
            _git_ref: &str,
            _path: &str,
        ) -> Result<Option<String>, GitError> {
            Ok(None)
        }
        async fn list_dir(
            &self,
            _repo: &Path,
            _git_ref: &str,
            _dir: &str,
        ) -> Result<Vec<String>, GitError> {
            Ok(vec![])
        }
        async fn list_tree_recursive(
            &self,
            _repo: &Path,
            _git_ref: &str,
            _dir: &str,
        ) -> Result<Vec<String>, GitError> {
            Ok(vec![])
        }
        async fn branch_exists(&self, _repo: &Path, _branch: &str) -> Result<bool, GitError> {
            Ok(true)
        }
        async fn is_ancestor(
            &self,
            _repo: &Path,
            _ancestor: &str,
            _commit: &str,
        ) -> Result<bool, GitError> {
            Ok(true)
        }
    }

    struct MockGitWriter;
    impl GitWriter for MockGitWriter {
        async fn commit_files(
            &self,
            _repo: &Path,
            _branch: &str,
            _files: &[(&str, &[u8])],
            _message: &str,
        ) -> Result<String, GitError> {
            Ok("abc123".into())
        }
        async fn commit_all(
            &self,
            _repo: &Path,
            _branch: &str,
            _files: &[(&str, &[u8])],
            _remove_dirs: &[&str],
            _message: &str,
        ) -> Result<String, GitError> {
            Ok("abc123".into())
        }
        async fn revert_head(&self, _repo: &Path, _branch: &str) -> Result<String, GitError> {
            Ok("abc123".into())
        }
    }

    struct MockGitMerger;
    impl GitMerger for MockGitMerger {
        async fn merge_no_ff(
            &self,
            _repo: &Path,
            _source: &str,
            _target: &str,
            _message: &str,
        ) -> Result<String, GitError> {
            Ok("abc123".into())
        }
        async fn squash_merge(
            &self,
            _repo: &Path,
            _source: &str,
            _target: &str,
            _message: &str,
        ) -> Result<String, GitError> {
            Ok("abc123".into())
        }
        async fn rebase_merge(
            &self,
            _repo: &Path,
            _source: &str,
            _target: &str,
        ) -> Result<String, GitError> {
            Ok("abc123".into())
        }
    }

    #[tokio::test]
    async fn mock_git_core_read_works() {
        let git = MockGitCoreRead;
        let sha = git.rev_parse(Path::new("/repo"), "HEAD").await.unwrap();
        assert_eq!(sha, "abc123");
        assert!(git.branch_exists(Path::new("/repo"), "main").await.unwrap());
    }

    #[tokio::test]
    async fn mock_git_writer_works() {
        let writer = MockGitWriter;
        let sha = writer
            .commit_files(Path::new("/repo"), "main", &[], "msg")
            .await
            .unwrap();
        assert_eq!(sha, "abc123");
    }

    #[tokio::test]
    async fn mock_git_merger_works() {
        let merger = MockGitMerger;
        let sha = merger
            .merge_no_ff(Path::new("/repo"), "feat", "main", "merge")
            .await
            .unwrap();
        assert_eq!(sha, "abc123");
    }
}
