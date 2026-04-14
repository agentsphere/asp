// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! DB-backed implementations of git traits.
//!
//! These implementations require `PgPool` for database queries. They implement
//! the traits defined in `traits.rs` that have no default implementation.
//!
//! - [`PgProjectResolver`] — resolve owner/repo path to a project
//! - [`PgBranchProtectionProvider`] — look up branch protection rules
//! - [`PgGitAuthenticator`] — authenticate via basic auth or SSH key
//! - [`PgGitAccessControl`] — check read/write access via RBAC
//! - [`PgPostReceiveHandler`] — side effects after push (generic over event sinks)

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::{GitError, Permission, PermissionChecker};

use crate::protection::BranchProtection;
use crate::traits::{
    BranchProtectionProvider, GitAccessControl, GitAuthenticator, PostReceiveHandler,
    ProjectResolver,
};
use crate::types::{GitUser, MrSyncEvent, PushEvent, ResolvedProject, TagEvent};

// ---------------------------------------------------------------------------
// 1. PgProjectResolver
// ---------------------------------------------------------------------------

/// Resolves owner/repo path segments to a project via Postgres.
pub struct PgProjectResolver<'a> {
    pool: &'a PgPool,
    repos_path: &'a std::path::Path,
}

impl<'a> PgProjectResolver<'a> {
    pub fn new(pool: &'a PgPool, repos_path: &'a std::path::Path) -> Self {
        Self { pool, repos_path }
    }
}

impl ProjectResolver for PgProjectResolver<'_> {
    async fn resolve(&self, owner: &str, repo: &str) -> Result<ResolvedProject, GitError> {
        let row = sqlx::query!(
            r#"SELECT p.id as "project_id!", p.owner_id as "owner_id!",
                      p.default_branch as "default_branch!", p.visibility as "visibility!"
               FROM projects p
               JOIN users u ON p.owner_id = u.id
               WHERE u.name = $1 AND p.name = $2 AND p.is_active = true"#,
            owner,
            repo,
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| GitError::Other(anyhow::anyhow!(e)))?
        .ok_or_else(|| GitError::NotFound(format!("{owner}/{repo}")))?;

        Ok(ResolvedProject {
            project_id: row.project_id,
            owner_id: row.owner_id,
            repo_disk_path: self.repos_path.join(owner).join(format!("{repo}.git")),
            default_branch: row.default_branch,
            visibility: row.visibility,
        })
    }
}

// ---------------------------------------------------------------------------
// 2. PgBranchProtectionProvider
// ---------------------------------------------------------------------------

/// Looks up branch protection rules from Postgres.
pub struct PgBranchProtectionProvider<'a> {
    pool: &'a PgPool,
}

impl<'a> PgBranchProtectionProvider<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

impl BranchProtectionProvider for PgBranchProtectionProvider<'_> {
    async fn get_protection(
        &self,
        project_id: Uuid,
        branch: &str,
    ) -> Result<Option<BranchProtection>, anyhow::Error> {
        let row = sqlx::query!(
            r#"SELECT id, pattern, require_pr, block_force_push, required_approvals,
                      dismiss_stale_reviews, required_checks, require_up_to_date,
                      allow_admin_bypass, merge_methods
               FROM branch_protection_rules
               WHERE project_id = $1 AND pattern = $2"#,
            project_id,
            branch,
        )
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|r| BranchProtection {
            id: r.id,
            project_id,
            pattern: r.pattern,
            require_pr: r.require_pr,
            block_force_push: r.block_force_push,
            required_approvals: r.required_approvals,
            dismiss_stale_reviews: r.dismiss_stale_reviews,
            required_checks: r.required_checks,
            require_up_to_date: r.require_up_to_date,
            allow_admin_bypass: r.allow_admin_bypass,
            merge_methods: r.merge_methods,
        }))
    }
}

// ---------------------------------------------------------------------------
// 3. PgGitAuthenticator
// ---------------------------------------------------------------------------

/// Authenticates git users via basic auth (API tokens) or SSH keys from Postgres.
pub struct PgGitAuthenticator<'a> {
    pool: &'a PgPool,
}

impl<'a> PgGitAuthenticator<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

impl GitAuthenticator for PgGitAuthenticator<'_> {
    async fn authenticate_basic(
        &self,
        username: &str,
        password: &str,
    ) -> Result<GitUser, GitError> {
        // Treat password as an API token
        let token_hash = platform_auth::token::hash_token(password);

        let row = sqlx::query!(
            r#"SELECT u.id as "user_id!", u.name as "user_name!",
                      t.project_id, t.scope_workspace_id, t.scopes
               FROM api_tokens t
               JOIN users u ON t.user_id = u.id
               WHERE t.token_hash = $1
                 AND u.name = $2
                 AND u.is_active = true
                 AND (t.expires_at IS NULL OR t.expires_at > now())"#,
            &token_hash,
            username,
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| GitError::Other(anyhow::anyhow!(e)))?
        .ok_or(GitError::Unauthorized)?;

        Ok(GitUser {
            user_id: row.user_id,
            user_name: row.user_name,
            ip_addr: None,
            boundary_project_id: row.project_id,
            boundary_workspace_id: row.scope_workspace_id,
            token_scopes: Some(row.scopes),
        })
    }

    async fn authenticate_ssh_key(&self, fingerprint: &str) -> Result<GitUser, GitError> {
        let row = sqlx::query!(
            r#"SELECT u.id as "user_id!", u.name as "user_name!"
               FROM users u
               JOIN user_ssh_keys sk ON u.id = sk.user_id
               WHERE sk.fingerprint = $1 AND u.is_active = true"#,
            fingerprint,
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| GitError::Other(anyhow::anyhow!(e)))?
        .ok_or(GitError::Unauthorized)?;

        Ok(GitUser {
            user_id: row.user_id,
            user_name: row.user_name,
            ip_addr: None,
            boundary_project_id: None,
            boundary_workspace_id: None,
            token_scopes: None,
        })
    }
}

// ---------------------------------------------------------------------------
// 4. PgGitAccessControl
// ---------------------------------------------------------------------------

/// Checks read/write access via the RBAC permission checker.
///
/// Generic over `P: PermissionChecker` to allow different permission
/// resolution strategies (DB-backed, cached, mock).
pub struct PgGitAccessControl<P: PermissionChecker> {
    perm_checker: P,
}

impl<P: PermissionChecker> PgGitAccessControl<P> {
    pub fn new(perm_checker: P) -> Self {
        Self { perm_checker }
    }
}

impl<P: PermissionChecker + 'static> GitAccessControl for PgGitAccessControl<P> {
    async fn check_read(&self, user: &GitUser, project: &ResolvedProject) -> Result<(), GitError> {
        // Public and internal repos are readable by any authenticated user
        if project.visibility == "public" || project.visibility == "internal" {
            return Ok(());
        }
        // Owner always has access
        if user.user_id == project.owner_id {
            return Ok(());
        }

        let allowed = self
            .perm_checker
            .has_permission_scoped(
                user.user_id,
                Some(project.project_id),
                Permission::ProjectRead,
                user.token_scopes.as_deref(),
            )
            .await
            .map_err(|e| GitError::Other(anyhow::anyhow!(e)))?;

        if !allowed {
            // Return NotFound to avoid leaking existence of private repos
            return Err(GitError::NotFound(String::new()));
        }
        Ok(())
    }

    async fn check_write(&self, user: &GitUser, project: &ResolvedProject) -> Result<(), GitError> {
        // Owner always has write access
        if user.user_id == project.owner_id {
            return Ok(());
        }

        let allowed = self
            .perm_checker
            .has_permission_scoped(
                user.user_id,
                Some(project.project_id),
                Permission::ProjectWrite,
                user.token_scopes.as_deref(),
            )
            .await
            .map_err(|e| GitError::Other(anyhow::anyhow!(e)))?;

        if !allowed {
            return Err(GitError::Forbidden);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 5. PgPostReceiveHandler
// ---------------------------------------------------------------------------

/// Trait for side effects triggered by post-receive events.
///
/// Allows different implementations (pipeline triggers, webhook dispatch,
/// MR status updates) to be composed without the handler knowing the details.
pub trait PostReceiveSideEffects: Send + Sync {
    /// Trigger a pipeline for the push event.
    fn trigger_pipeline(
        &self,
        project_id: Uuid,
        branch: &str,
        commit_sha: Option<&str>,
        repo_path: &std::path::Path,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Fire webhooks for a project event.
    fn fire_webhooks(
        &self,
        project_id: Uuid,
        event: &str,
        payload: &serde_json::Value,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Post-receive handler backed by Postgres with pluggable side effects.
pub struct PgPostReceiveHandler<S: PostReceiveSideEffects> {
    pool: PgPool,
    side_effects: S,
}

impl<S: PostReceiveSideEffects> PgPostReceiveHandler<S> {
    pub fn new(pool: PgPool, side_effects: S) -> Self {
        Self { pool, side_effects }
    }
}

impl<S: PostReceiveSideEffects + 'static> PostReceiveHandler for PgPostReceiveHandler<S> {
    async fn on_push(&self, params: &PushEvent) -> Result<(), anyhow::Error> {
        // Fire push webhook
        self.side_effects
            .fire_webhooks(
                params.project_id,
                "push",
                &serde_json::json!({
                    "ref": format!("refs/heads/{}", params.branch),
                    "after": params.commit_sha,
                    "pusher": params.user_name,
                }),
            )
            .await;

        // Trigger pipeline if applicable
        if let Err(e) = self
            .side_effects
            .trigger_pipeline(
                params.project_id,
                &params.branch,
                params.commit_sha.as_deref(),
                &params.repo_path,
            )
            .await
        {
            tracing::debug!(
                error = %e,
                project_id = %params.project_id,
                branch = %params.branch,
                "pipeline trigger skipped"
            );
        }

        // Update any open MRs that match the pushed branch
        let _ = sqlx::query!(
            "UPDATE merge_requests SET updated_at = now()
             WHERE project_id = $1 AND source_branch = $2 AND status = 'open'",
            params.project_id,
            params.branch,
        )
        .execute(&self.pool)
        .await;

        Ok(())
    }

    async fn on_tag(&self, params: &TagEvent) -> Result<(), anyhow::Error> {
        self.side_effects
            .fire_webhooks(
                params.project_id,
                "tag",
                &serde_json::json!({
                    "ref": format!("refs/tags/{}", params.tag_name),
                    "after": params.commit_sha,
                    "pusher": params.user_name,
                }),
            )
            .await;

        Ok(())
    }

    async fn on_mr_sync(&self, params: &MrSyncEvent) -> Result<(), anyhow::Error> {
        // Update MR head SHA
        let _ = sqlx::query!(
            "UPDATE merge_requests SET head_sha = $1, updated_at = now()
             WHERE project_id = $2 AND source_branch = $3 AND status = 'open'",
            params.commit_sha,
            params.project_id,
            params.branch,
        )
        .execute(&self.pool)
        .await;

        // Fire MR sync webhook
        self.side_effects
            .fire_webhooks(
                params.project_id,
                "mr",
                &serde_json::json!({
                    "action": "synchronize",
                    "branch": params.branch,
                    "head_sha": params.commit_sha,
                }),
            )
            .await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_resolver_is_constructible() {
        fn make(pool: &PgPool) -> PgProjectResolver<'_> {
            PgProjectResolver::new(pool, std::path::Path::new("/repos"))
        }
        let _ = make as fn(&PgPool) -> PgProjectResolver<'_>;
    }

    #[test]
    fn branch_protection_provider_is_constructible() {
        fn make(pool: &PgPool) -> PgBranchProtectionProvider<'_> {
            PgBranchProtectionProvider::new(pool)
        }
        let _ = make as fn(&PgPool) -> PgBranchProtectionProvider<'_>;
    }

    #[test]
    fn git_authenticator_is_constructible() {
        fn make(pool: &PgPool) -> PgGitAuthenticator<'_> {
            PgGitAuthenticator::new(pool)
        }
        let _ = make as fn(&PgPool) -> PgGitAuthenticator<'_>;
    }

    #[test]
    fn git_access_control_compiles_with_mock() {
        struct MockChecker;
        impl PermissionChecker for MockChecker {
            async fn has_permission(
                &self,
                _user_id: Uuid,
                _project_id: Option<Uuid>,
                _perm: Permission,
            ) -> anyhow::Result<bool> {
                Ok(true)
            }
            async fn has_permission_scoped(
                &self,
                _user_id: Uuid,
                _project_id: Option<Uuid>,
                _perm: Permission,
                _token_scopes: Option<&[String]>,
            ) -> anyhow::Result<bool> {
                Ok(true)
            }
        }
        let _control = PgGitAccessControl::new(MockChecker);
    }

    #[test]
    fn post_receive_handler_compiles_with_mock() {
        struct MockSideEffects;
        impl PostReceiveSideEffects for MockSideEffects {
            async fn trigger_pipeline(
                &self,
                _project_id: Uuid,
                _branch: &str,
                _commit_sha: Option<&str>,
                _repo_path: &std::path::Path,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn fire_webhooks(
                &self,
                _project_id: Uuid,
                _event: &str,
                _payload: &serde_json::Value,
            ) {
            }
        }
        // Verify the generic compiles (needs a PgPool at runtime)
        let _ctor = |pool: PgPool| PgPostReceiveHandler::new(pool, MockSideEffects);
    }
}
