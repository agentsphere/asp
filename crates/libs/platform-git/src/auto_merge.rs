// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Auto-merge handler for merge requests.
//!
//! Implements [`MergeRequestHandler`] by querying open MRs with `auto_merge = true`
//! and attempting to merge each one via a [`GitMerger`] implementation.

use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::{GitMerger, MergeRequestHandler};

/// Auto-merge handler backed by Postgres + git merge operations.
///
/// Generic over `G: GitMerger` to allow different merge strategies
/// (CLI-based, in-memory for tests, etc.).
pub struct AutoMergeHandler<G: GitMerger> {
    pool: PgPool,
    git_merger: G,
    repos_path: PathBuf,
}

impl<G: GitMerger> AutoMergeHandler<G> {
    pub fn new(pool: PgPool, git_merger: G, repos_path: PathBuf) -> Self {
        Self {
            pool,
            git_merger,
            repos_path,
        }
    }
}

impl<G: GitMerger + 'static> MergeRequestHandler for AutoMergeHandler<G> {
    async fn try_auto_merge(&self, project_id: Uuid) {
        let mrs = match sqlx::query!(
            r#"SELECT number, source_branch, target_branch, auto_merge_method, title
               FROM merge_requests
               WHERE project_id = $1 AND status = 'open' AND auto_merge = true"#,
            project_id,
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(error = %e, %project_id, "failed to query auto-merge MRs");
                return;
            }
        };

        // Look up the repo path
        let repo_info = match sqlx::query!(
            r#"SELECT u.name as "owner_name!", p.name as "project_name!"
               FROM projects p
               JOIN users u ON p.owner_id = u.id
               WHERE p.id = $1 AND p.is_active = true"#,
            project_id,
        )
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(row)) => row,
            Ok(None) => {
                tracing::warn!(%project_id, "project not found for auto-merge");
                return;
            }
            Err(e) => {
                tracing::error!(error = %e, %project_id, "failed to look up project for auto-merge");
                return;
            }
        };

        let repo_path = self
            .repos_path
            .join(&repo_info.owner_name)
            .join(format!("{}.git", repo_info.project_name));

        for mr in mrs {
            let method = mr.auto_merge_method.as_deref().unwrap_or("merge");
            let msg = format!(
                "Merge branch '{}' into '{}'\n\nMerge request !{}: {}",
                mr.source_branch, mr.target_branch, mr.number, mr.title,
            );

            let result = match method {
                "squash" => {
                    self.git_merger
                        .squash_merge(&repo_path, &mr.source_branch, &mr.target_branch, &msg)
                        .await
                }
                "rebase" => {
                    self.git_merger
                        .rebase_merge(&repo_path, &mr.source_branch, &mr.target_branch)
                        .await
                }
                _ => {
                    // Default: --no-ff merge
                    self.git_merger
                        .merge_no_ff(&repo_path, &mr.source_branch, &mr.target_branch, &msg)
                        .await
                }
            };

            match result {
                Ok(sha) => {
                    // Mark the MR as merged
                    if let Err(e) = sqlx::query!(
                        "UPDATE merge_requests SET status = 'merged', merged_at = now(), auto_merge = false
                         WHERE project_id = $1 AND number = $2",
                        project_id,
                        mr.number,
                    )
                    .execute(&self.pool)
                    .await
                    {
                        tracing::error!(error = %e, %project_id, number = mr.number, "failed to update MR status after merge");
                    } else {
                        tracing::info!(%project_id, number = mr.number, %sha, "auto-merge succeeded");
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, %project_id, number = mr.number, "auto-merge not ready");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_merge_handler_is_constructible() {
        // Verify generic constraints compile
        struct NoopMerger;
        impl platform_types::GitMerger for NoopMerger {
            async fn merge_no_ff(
                &self,
                _repo: &std::path::Path,
                _src: &str,
                _dst: &str,
                _msg: &str,
            ) -> Result<String, platform_types::GitError> {
                Ok("abc123".into())
            }
            async fn squash_merge(
                &self,
                _repo: &std::path::Path,
                _src: &str,
                _dst: &str,
                _msg: &str,
            ) -> Result<String, platform_types::GitError> {
                Ok("abc123".into())
            }
            async fn rebase_merge(
                &self,
                _repo: &std::path::Path,
                _src: &str,
                _dst: &str,
            ) -> Result<String, platform_types::GitError> {
                Ok("abc123".into())
            }
        }

        // Verify the handler can be constructed
        let _ctor = |pool: PgPool| AutoMergeHandler::new(pool, NoopMerger, PathBuf::from("/repos"));
    }
}
