// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Registry access control: repository lookup, permission checks, and tag operations.

use platform_types::{Permission, PermissionChecker, WorkspaceMembershipChecker};
use uuid::Uuid;

use crate::error::RegistryError;

/// Authenticated registry user — plain data struct (no `FromRequestParts` impl).
/// The concrete axum extractor lives in `src/registry/auth.rs`.
#[derive(Debug, Clone)]
pub struct RegistryUser {
    pub user_id: Uuid,
    pub user_name: String,
    /// Hard project boundary from API token.
    pub boundary_project_id: Option<Uuid>,
    /// Hard workspace boundary from API token.
    pub boundary_workspace_id: Option<Uuid>,
    /// When non-NULL, limits which image name:tag this token can push to (glob pattern).
    pub registry_tag_pattern: Option<String>,
    /// Token permission scopes (None = password auth, Some = API token auth).
    pub token_scopes: Option<Vec<String>>,
}

/// Resolved repository access — returned by `resolve_repo_with_access`.
#[derive(Debug)]
pub struct RepoAccess {
    pub repository_id: Uuid,
    pub project_id: Option<Uuid>,
}

/// Resolved project info from a repository lookup.
struct RepoProject {
    repository_id: Uuid,
    project_id: Option<Uuid>,
    owner_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    visibility: Option<String>,
}

/// Look up a repository by name, `LEFT JOIN`ing to its parent project.
///
/// System/global repos (e.g. `platform-runner`) have `project_id = NULL`.
/// The URL path segment is a **repository** name (which may differ from
/// the project name for project-scoped repos).
async fn lookup_repo_and_project(
    pool: &sqlx::PgPool,
    name: &str,
) -> Result<Option<RepoProject>, sqlx::Error> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Option<Uuid>,
            Option<Uuid>,
            Option<Uuid>,
            Option<String>,
        ),
    >(
        r"SELECT r.id,
                  r.project_id,
                  p.owner_id,
                  p.workspace_id,
                  p.visibility
           FROM registry_repositories r
           LEFT JOIN projects p ON p.id = r.project_id AND p.is_active = true
           WHERE r.name = $1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(repository_id, project_id, owner_id, workspace_id, visibility)| RepoProject {
            repository_id,
            project_id,
            owner_id,
            workspace_id,
            visibility,
        },
    ))
}

/// Resolve a repository name to a project, checking ownership and permissions.
///
/// System/global repos (`project_id = NULL`) allow pull but deny push.
/// Project-scoped repos look up by repository name first. Falls back to
/// project-name lookup with lazy repo creation for push operations.
///
/// Returns 404 (not 403) if user lacks access to avoid leaking existence.
#[allow(clippy::too_many_lines)]
pub async fn resolve_repo_with_access(
    pool: &sqlx::PgPool,
    permission_checker: &impl PermissionChecker,
    workspace_checker: &impl WorkspaceMembershipChecker,
    user: &RegistryUser,
    name: &str,
    need_push: bool,
) -> Result<RepoAccess, RegistryError> {
    // 1. Try to find existing repository → project
    let resolved = lookup_repo_and_project(pool, name).await?;

    // Handle system/global repos (project_id IS NULL): pull OK, push denied
    if let Some(ref rp) = resolved
        && rp.project_id.is_none()
    {
        if need_push {
            return Err(RegistryError::Denied);
        }
        return Ok(RepoAccess {
            repository_id: rp.repository_id,
            project_id: None,
        });
    }

    let (repository_id, project_id, owner_id, workspace_id) = if let Some(rp) = resolved {
        // project_id is Some here (system repos handled above)
        (
            Some(rp.repository_id),
            rp.project_id
                .ok_or(RegistryError::Internal(anyhow::anyhow!(
                    "project_id unexpectedly NULL for project-scoped repo"
                )))?,
            rp.owner_id.ok_or(RegistryError::Internal(anyhow::anyhow!(
                "owner_id unexpectedly NULL for project-scoped repo"
            )))?,
            rp.workspace_id
                .ok_or(RegistryError::Internal(anyhow::anyhow!(
                    "workspace_id unexpectedly NULL for project-scoped repo"
                )))?,
        )
    } else if need_push {
        // No repo found — for push, fall back to project-name lookup + lazy-create.
        // For namespaced names like "project/dev", use the first segment as project name.
        let project_lookup_name = if let Some(slash) = name.find('/') {
            &name[..slash]
        } else {
            name
        };
        let row = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
            r"SELECT id, owner_id, workspace_id
               FROM projects
               WHERE name = $1 AND is_active = true",
        )
        .bind(project_lookup_name)
        .fetch_optional(pool)
        .await?
        .ok_or(RegistryError::NameUnknown)?;
        (None, row.0, row.1, row.2)
    } else {
        // Pull with no existing repo — nothing to pull
        return Err(RegistryError::NameUnknown);
    };

    // Enforce hard project boundary from API token
    if let Some(boundary_pid) = user.boundary_project_id
        && boundary_pid != project_id
    {
        return Err(RegistryError::NameUnknown);
    }

    // Enforce hard workspace boundary from API token
    if let Some(boundary_wid) = user.boundary_workspace_id
        && workspace_id != boundary_wid
    {
        return Err(RegistryError::NameUnknown);
    }

    // 2. Owner always has full access
    let is_owner = owner_id == user.user_id;

    if !is_owner {
        // 3. Check workspace membership
        let is_workspace_member = workspace_checker
            .is_member(workspace_id, user.user_id)
            .await
            .map_err(|e| RegistryError::Internal(anyhow::anyhow!("{e}")))?;

        // Workspace members get implicit pull access.
        // Push always requires explicit RBAC permission.
        let needs_rbac_check = need_push || !is_workspace_member;

        if needs_rbac_check {
            let perm = if need_push {
                Permission::RegistryPush
            } else {
                Permission::RegistryPull
            };
            let allowed = permission_checker
                .has_permission_scoped(
                    user.user_id,
                    Some(project_id),
                    perm,
                    user.token_scopes.as_deref(),
                )
                .await
                .map_err(RegistryError::Internal)?;

            if !allowed {
                return Err(RegistryError::NameUnknown); // 404, not 403
            }
        }
    }

    // 4. Get or create the repository
    let repository_id = if let Some(id) = repository_id {
        id
    } else {
        // Lazy-create on first push (only reachable when need_push && no repo found)
        let id = Uuid::new_v4();
        let row: (Uuid,) = sqlx::query_as(
            r"INSERT INTO registry_repositories (id, project_id, name)
               VALUES ($1, $2, $3)
               ON CONFLICT (name) DO UPDATE SET updated_at = now()
               RETURNING id",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .fetch_one(pool)
        .await?;
        row.0
    };

    Ok(RepoAccess {
        repository_id,
        project_id: Some(project_id),
    })
}

/// Resolve a repository for anonymous or authenticated access.
///
/// When `user` is `None`, only public projects are accessible (pull only).
/// When `user` is `Some`, delegates to the full `resolve_repo_with_access`.
pub async fn resolve_repo_with_optional_access(
    pool: &sqlx::PgPool,
    permission_checker: &impl PermissionChecker,
    workspace_checker: &impl WorkspaceMembershipChecker,
    user: Option<&RegistryUser>,
    name: &str,
    need_push: bool,
) -> Result<RepoAccess, RegistryError> {
    if let Some(user) = user {
        return resolve_repo_with_access(
            pool,
            permission_checker,
            workspace_checker,
            user,
            name,
            need_push,
        )
        .await;
    }

    // Anonymous access: push is never allowed
    if need_push {
        return Err(RegistryError::Unauthorized);
    }

    // Look up repository → project (must exist and be public or system)
    let rp = lookup_repo_and_project(pool, name)
        .await?
        .ok_or(RegistryError::NameUnknown)?;

    // System repos (project_id IS NULL) are publicly pullable
    if rp.project_id.is_none() {
        return Ok(RepoAccess {
            repository_id: rp.repository_id,
            project_id: None,
        });
    }

    if rp.visibility.as_deref() != Some("public") {
        // Return 401 (not 404) so containerd/Docker retries with credentials
        // from imagePullSecrets. Returning 404 would make it give up immediately.
        return Err(RegistryError::Unauthorized);
    }

    Ok(RepoAccess {
        repository_id: rp.repository_id,
        project_id: rp.project_id,
    })
}

/// Check if an image reference (e.g. `"myapp-dev:session-abc"`) matches a glob pattern.
///
/// The pattern supports `*` as a wildcard matching any sequence of characters.
/// Returns `true` if pattern is `None` (no restriction).
pub fn matches_tag_pattern(image_ref: &str, pattern: &str) -> bool {
    glob_match(pattern, image_ref)
}

/// Simple glob match: `*` matches any sequence of characters. No other wildcards.
pub fn glob_match(pattern: &str, input: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == input;
    }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = input[pos..].find(part) {
            if i == 0 && found != 0 {
                return false; // first segment must be a prefix
            }
            pos += found + part.len();
        } else {
            return false;
        }
    }

    // If pattern doesn't end with *, input must be fully consumed
    if !pattern.ends_with('*') {
        return pos == input.len();
    }

    true
}

/// Copy a manifest from one tag to another within the same repository.
///
/// Metadata-only — blobs are shared via content-addressable storage.
/// Returns error if `dest_tag` already exists (immutable alias tags).
pub async fn copy_tag(
    pool: &sqlx::PgPool,
    repo_name: &str,
    source_tag: &str,
    dest_tag: &str,
) -> Result<(), RegistryError> {
    // Look up repository
    let repo: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM registry_repositories WHERE name = $1")
            .bind(repo_name)
            .fetch_optional(pool)
            .await?
            .ok_or(RegistryError::NameUnknown)?;

    let repo = repo.ok_or(RegistryError::NameUnknown)?;

    // Get digest for source tag
    let source_digest: Option<String> = sqlx::query_scalar(
        "SELECT manifest_digest FROM registry_tags WHERE repository_id = $1 AND name = $2",
    )
    .bind(repo)
    .bind(source_tag)
    .fetch_optional(pool)
    .await?
    .ok_or(RegistryError::ManifestUnknown)?;

    let source_digest = source_digest.ok_or(RegistryError::ManifestUnknown)?;

    // Check dest_tag doesn't already exist
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT manifest_digest FROM registry_tags WHERE repository_id = $1 AND name = $2",
    )
    .bind(repo)
    .bind(dest_tag)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Err(RegistryError::TagExists(dest_tag.to_string()));
    }

    // Create tag pointing to the same digest
    sqlx::query(
        "INSERT INTO registry_tags (repository_id, name, manifest_digest) VALUES ($1, $2, $3)",
    )
    .bind(repo)
    .bind(dest_tag)
    .bind(&source_digest)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_tag_pattern_exact() {
        assert!(matches_tag_pattern(
            "myapp-dev:session-abc",
            "myapp-dev:session-*"
        ));
    }

    #[test]
    fn matches_tag_pattern_rejects_other_tag() {
        assert!(!matches_tag_pattern("myapp:latest", "myapp-dev:session-*"));
    }

    #[test]
    fn matches_tag_pattern_rejects_other_repo() {
        assert!(!matches_tag_pattern(
            "other-dev:session-abc",
            "myapp-dev:session-*"
        ));
    }

    #[test]
    fn matches_tag_pattern_wildcard_suffix() {
        assert!(matches_tag_pattern(
            "myapp-dev:session-abc12345-build1",
            "myapp-dev:session-abc12345-*"
        ));
    }

    #[test]
    fn matches_tag_pattern_no_wildcard_exact() {
        assert!(matches_tag_pattern("myapp:v1", "myapp:v1"));
        assert!(!matches_tag_pattern("myapp:v2", "myapp:v1"));
    }

    #[test]
    fn agent_tag_pattern_matches_app_image() {
        assert!(matches_tag_pattern(
            "myproject/session-abc12345-app:latest",
            "myproject/session-abc12345-*"
        ));
    }

    #[test]
    fn agent_tag_pattern_matches_test_image() {
        assert!(matches_tag_pattern(
            "myproject/session-abc12345-test:v2",
            "myproject/session-abc12345-*"
        ));
    }

    #[test]
    fn agent_tag_pattern_rejects_other_project() {
        assert!(!matches_tag_pattern(
            "otherproject/session-abc12345-app:latest",
            "myproject/session-abc12345-*"
        ));
    }

    #[test]
    fn agent_tag_pattern_rejects_other_session() {
        assert!(!matches_tag_pattern(
            "myproject/session-zzz99999-app:latest",
            "myproject/session-abc12345-*"
        ));
    }

    #[test]
    fn agent_tag_pattern_rejects_no_session_prefix() {
        assert!(!matches_tag_pattern(
            "myproject/app:latest",
            "myproject/session-abc12345-*"
        ));
    }

    #[test]
    fn glob_match_empty_pattern_and_input() {
        assert!(glob_match("", ""));
    }

    #[test]
    fn glob_match_star_only_matches_anything() {
        assert!(glob_match("*", "anything-at-all"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_match_multiple_stars() {
        assert!(glob_match("*foo*bar*", "xxxfooyyybarz"));
        assert!(glob_match("*foo*bar*", "foobar"));
        assert!(!glob_match("*foo*bar*", "foobaz"));
    }

    #[test]
    fn glob_match_leading_star() {
        assert!(glob_match("*.tar", "image.tar"));
        assert!(glob_match("*.tar", ".tar"));
        assert!(!glob_match("*.tar", "image.zip"));
    }

    #[test]
    fn glob_match_trailing_star() {
        assert!(glob_match("prefix*", "prefix-anything"));
        assert!(glob_match("prefix*", "prefix"));
        assert!(!glob_match("prefix*", "other"));
    }

    #[test]
    fn glob_match_no_star_must_be_exact() {
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exacta"));
        assert!(!glob_match("exact", "exac"));
    }

    #[test]
    fn glob_match_pattern_must_anchor_start() {
        assert!(glob_match("abc*", "abcdef"));
        assert!(!glob_match("abc*", "xabcdef"));
    }

    #[test]
    fn glob_match_pattern_must_anchor_end() {
        assert!(glob_match("*abc", "xyzabc"));
        assert!(!glob_match("*abc", "xyzabcd"));
    }

    #[test]
    fn glob_match_middle_star() {
        assert!(glob_match("a*z", "az"));
        assert!(glob_match("a*z", "abcz"));
        assert!(!glob_match("a*z", "abcza"));
    }

    #[test]
    fn repo_access_struct() {
        let access = RepoAccess {
            repository_id: Uuid::nil(),
            project_id: Some(Uuid::nil()),
        };
        assert_eq!(access.repository_id, Uuid::nil());
        assert_eq!(access.project_id, Some(Uuid::nil()));
    }

    #[test]
    fn repo_access_no_project() {
        let access = RepoAccess {
            repository_id: Uuid::new_v4(),
            project_id: None,
        };
        assert!(access.project_id.is_none());
    }

    #[test]
    fn glob_match_empty_pattern_nonempty_input() {
        assert!(!glob_match("", "something"));
    }

    #[test]
    fn glob_match_nonempty_pattern_empty_input() {
        assert!(!glob_match("abc", ""));
    }

    #[test]
    fn glob_match_star_star() {
        assert!(glob_match("**", "anything"));
        assert!(glob_match("**", ""));
    }

    #[test]
    fn glob_match_consecutive_stars_with_text() {
        assert!(glob_match("a**b", "ab"));
        assert!(glob_match("a**b", "axxb"));
    }

    #[test]
    fn glob_match_star_at_both_ends() {
        assert!(glob_match("*middle*", "start_middle_end"));
        assert!(glob_match("*middle*", "middle"));
        assert!(!glob_match("*middle*", "mid"));
    }

    #[test]
    fn glob_match_single_char_pattern() {
        assert!(glob_match("a", "a"));
        assert!(!glob_match("a", "b"));
        assert!(!glob_match("a", ""));
    }

    #[test]
    fn glob_match_pattern_longer_than_input() {
        assert!(!glob_match("abcdef", "abc"));
    }

    #[test]
    fn matches_tag_pattern_empty_pattern_empty_ref() {
        assert!(matches_tag_pattern("", ""));
    }

    #[test]
    fn matches_tag_pattern_star_matches_empty() {
        assert!(matches_tag_pattern("", "*"));
    }

    #[test]
    fn matches_tag_pattern_colon_in_pattern() {
        assert!(matches_tag_pattern("repo:tag", "repo:*"));
        assert!(matches_tag_pattern("repo:v1.0.0", "repo:v*"));
    }

    #[test]
    fn matches_tag_pattern_slash_in_pattern() {
        assert!(matches_tag_pattern("ns/repo:tag", "ns/repo:*"));
        assert!(!matches_tag_pattern("other/repo:tag", "ns/repo:*"));
    }

    #[test]
    fn registry_user_fields() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "alice".into(),
            boundary_project_id: None,
            boundary_workspace_id: None,
            registry_tag_pattern: Some("myapp:*".into()),
            token_scopes: Some(vec!["registry:push".into()]),
        };
        assert_eq!(user.user_name, "alice");
        assert_eq!(user.registry_tag_pattern.as_deref(), Some("myapp:*"));
    }

    #[test]
    fn registry_user_debug() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "bob".into(),
            boundary_project_id: None,
            boundary_workspace_id: None,
            registry_tag_pattern: None,
            token_scopes: None,
        };
        let debug = format!("{user:?}");
        assert!(debug.contains("bob"));
    }

    #[test]
    fn registry_user_clone() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "charlie".into(),
            boundary_project_id: Some(Uuid::nil()),
            boundary_workspace_id: Some(Uuid::nil()),
            registry_tag_pattern: Some("proj:*".into()),
            token_scopes: Some(vec!["registry:pull".into()]),
        };
        let cloned = user.clone();
        assert_eq!(cloned.user_id, user.user_id);
        assert_eq!(cloned.user_name, user.user_name);
        assert_eq!(cloned.registry_tag_pattern, user.registry_tag_pattern);
        assert_eq!(cloned.token_scopes, user.token_scopes);
    }
}
