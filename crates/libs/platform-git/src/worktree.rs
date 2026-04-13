// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! `CliGitMerger` and `CliGitWorktreeWriter` — merge and write operations via worktrees.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::GitError;
use crate::lock;
use crate::traits::{GitMerger, GitWorktreeWriter};

// ---------------------------------------------------------------------------
// CliGitMerger
// ---------------------------------------------------------------------------

/// Default [`GitMerger`] implementation using temporary worktrees.
pub struct CliGitMerger;

impl GitMerger for CliGitMerger {
    async fn merge_no_ff(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
        message: &str,
    ) -> Result<String, GitError> {
        let _lock = lock::repo_lock(repo).await;
        let worktree_dir = repo.join(format!("_merge_worktree_{}", Uuid::new_v4()));

        // Add temporary worktree on target branch
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_dir.to_str().unwrap_or_default(),
                target,
            ],
        )
        .await?;

        // Merge source into target with --no-ff
        let merge_result =
            run_git_with_env(&worktree_dir, &["merge", "--no-ff", source, "-m", message]).await;

        // Get the merge commit SHA before cleanup
        let sha = if merge_result.is_ok() {
            run_git(&worktree_dir, &["rev-parse", "HEAD"])
                .await
                .map(|s| s.trim().to_string())
                .ok()
        } else {
            None
        };

        cleanup_worktree(repo, &worktree_dir).await;
        merge_result?;

        sha.ok_or_else(|| GitError::CommandFailed {
            command: "git rev-parse HEAD".into(),
            stderr: "failed to get merge commit SHA".into(),
        })
    }

    async fn squash_merge(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
        message: &str,
    ) -> Result<String, GitError> {
        let _lock = lock::repo_lock(repo).await;
        let worktree_dir = repo.join(format!("_squash_worktree_{}", Uuid::new_v4()));

        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_dir.to_str().unwrap_or_default(),
                target,
            ],
        )
        .await?;

        let squash_result = run_git_with_env(&worktree_dir, &["merge", "--squash", source]).await;

        let commit_result = if squash_result.is_ok() {
            run_git_with_env(&worktree_dir, &["commit", "-m", message]).await
        } else {
            squash_result
        };

        let sha = if commit_result.is_ok() {
            run_git(&worktree_dir, &["rev-parse", "HEAD"])
                .await
                .map(|s| s.trim().to_string())
                .ok()
        } else {
            None
        };

        cleanup_worktree(repo, &worktree_dir).await;
        commit_result?;

        sha.ok_or_else(|| GitError::CommandFailed {
            command: "git rev-parse HEAD".into(),
            stderr: "failed to get squash commit SHA".into(),
        })
    }

    async fn rebase_merge(
        &self,
        repo: &Path,
        source: &str,
        target: &str,
    ) -> Result<String, GitError> {
        let _lock = lock::repo_lock(repo).await;
        let worktree_dir = repo.join(format!("_rebase_worktree_{}", Uuid::new_v4()));

        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_dir.to_str().unwrap_or_default(),
                target,
            ],
        )
        .await?;

        let ff_result = run_git_with_env(&worktree_dir, &["merge", "--ff-only", source]).await;

        let sha = if ff_result.is_ok() {
            run_git(&worktree_dir, &["rev-parse", "HEAD"])
                .await
                .map(|s| s.trim().to_string())
                .ok()
        } else {
            None
        };

        cleanup_worktree(repo, &worktree_dir).await;
        ff_result?;

        sha.ok_or_else(|| GitError::CommandFailed {
            command: "git rev-parse HEAD".into(),
            stderr: "failed to get rebase commit SHA".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// CliGitWorktreeWriter
// ---------------------------------------------------------------------------

/// Default [`GitWorktreeWriter`] implementation using temporary worktrees.
pub struct CliGitWorktreeWriter;

impl GitWorktreeWriter for CliGitWorktreeWriter {
    async fn commit_files(
        &self,
        repo: &Path,
        branch: &str,
        files: &[(&str, &[u8])],
        message: &str,
    ) -> Result<String, GitError> {
        let _lock = lock::repo_lock(repo).await;

        // Ensure branch exists
        ensure_branch_exists(repo, branch).await?;

        let worktree_dir = repo.join(format!("_file_worktree_{}", Uuid::new_v4()));

        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_dir.to_str().unwrap_or_default(),
                branch,
            ],
        )
        .await?;

        let result = async {
            // Write all files
            for (path, content) in files {
                let dest = worktree_dir.join(path);
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(GitError::Io)?;
                }
                tokio::fs::write(&dest, content)
                    .await
                    .map_err(GitError::Io)?;

                // Stage each file
                let _ = run_git(&worktree_dir, &["add", path]).await;
            }

            // Check if there are staged changes
            let diff_output = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&worktree_dir)
                .args(["diff", "--cached", "--quiet"])
                .status()
                .await;

            if diff_output.map(|s| s.success()).unwrap_or(false) {
                // No changes — return current HEAD
                let sha = run_git(&worktree_dir, &["rev-parse", "HEAD"]).await?;
                return Ok(sha.trim().to_string());
            }

            // Commit
            run_git_with_env(&worktree_dir, &["commit", "-m", message]).await?;

            let sha = run_git(&worktree_dir, &["rev-parse", "HEAD"]).await?;
            Ok(sha.trim().to_string())
        }
        .await;

        cleanup_worktree(repo, &worktree_dir).await;
        result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Ensure a branch exists in the bare repo, creating it as an orphan if needed.
async fn ensure_branch_exists(repo: &Path, branch: &str) -> Result<(), GitError> {
    let check = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .output()
        .await
        .map_err(GitError::Io)?;

    if check.status.success() {
        return Ok(());
    }

    let tmp_wt = repo.join(format!("_init_worktree_{}", Uuid::new_v4()));
    let wt_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add", "--orphan", "-b", branch])
        .arg(&tmp_wt)
        .output()
        .await;

    if let Ok(ref out) = wt_output
        && !out.status.success()
    {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&out.stderr),
            "ensure_branch_exists: worktree add --orphan failed"
        );
    }

    let commit_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&tmp_wt)
        .args(["commit", "--allow-empty", "-m", "initial commit"])
        .output()
        .await;

    if let Ok(ref out) = commit_output
        && !out.status.success()
    {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&out.stderr),
            "ensure_branch_exists: initial commit failed"
        );
    }

    cleanup_worktree(repo, &tmp_wt).await;
    Ok(())
}

/// Remove a git worktree and its directory.
async fn cleanup_worktree(repo: &Path, worktree_dir: &PathBuf) {
    let _ = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_dir)
        .output()
        .await;

    let _ = tokio::fs::remove_dir_all(worktree_dir).await;
}

/// Run a git command in a given directory.
async fn run_git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .map_err(GitError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a git command with Platform author/committer environment.
async fn run_git_with_env(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_AUTHOR_NAME", "Platform")
        .env("GIT_AUTHOR_EMAIL", "platform@localhost")
        .env("GIT_COMMITTER_NAME", "Platform")
        .env("GIT_COMMITTER_EMAIL", "platform@localhost")
        .args(args)
        .output()
        .await
        .map_err(GitError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plumbing::CliGitRepoManager;
    use crate::templates::TemplateFile;
    use crate::traits::GitRepoManager;

    /// Helper to create a bare repo with an initial commit.
    async fn create_test_repo(tmp: &Path) -> PathBuf {
        let mgr = CliGitRepoManager;
        let files = vec![TemplateFile {
            path: "README.md",
            content: "# Test".into(),
        }];
        mgr.init_bare_with_files(tmp, "test", "repo", "main", &files)
            .await
            .expect("failed to create test repo")
    }

    #[tokio::test]
    async fn worktree_writer_commit_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = create_test_repo(tmp.path()).await;

        let writer = CliGitWorktreeWriter;
        let result = writer
            .commit_files(
                &repo,
                "main",
                &[("test.txt", b"hello world")],
                "add test.txt",
            )
            .await;

        assert!(result.is_ok(), "commit_files should succeed: {result:?}");
        let sha = result.unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[tokio::test]
    async fn worktree_writer_commit_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = create_test_repo(tmp.path()).await;

        let writer = CliGitWorktreeWriter;
        let result = writer
            .commit_files(
                &repo,
                "main",
                &[
                    ("src/main.rs", b"fn main() {}"),
                    ("src/lib.rs", b"pub mod app;"),
                ],
                "add source files",
            )
            .await;

        assert!(
            result.is_ok(),
            "commit nested files should succeed: {result:?}"
        );
    }

    #[tokio::test]
    async fn merger_merge_no_ff() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = create_test_repo(tmp.path()).await;

        // Create a feature branch with a commit
        let writer = CliGitWorktreeWriter;

        // First, create the feature branch from main
        let _ = run_git(&repo, &["branch", "feature", "main"]).await;

        let _ = writer
            .commit_files(
                &repo,
                "feature",
                &[("feature.txt", b"feature content")],
                "add feature",
            )
            .await
            .unwrap();

        let merger = CliGitMerger;
        let result = merger
            .merge_no_ff(&repo, "feature", "main", "Merge feature into main")
            .await;

        assert!(result.is_ok(), "merge_no_ff should succeed: {result:?}");
        let sha = result.unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[tokio::test]
    async fn cleanup_worktree_nonexistent_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let nonexistent = tmp.path().join("does_not_exist");
        // Should not panic or error — just silently succeed
        cleanup_worktree(tmp.path(), &nonexistent).await;
    }
}
