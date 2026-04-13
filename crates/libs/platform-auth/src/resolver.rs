// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::collections::HashSet;
use std::str::FromStr;

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::valkey;
use platform_types::{Permission, PermissionChecker, PermissionResolver};

static CACHE_TTL: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// Set the permission cache TTL (seconds). Call once at startup.
pub fn set_cache_ttl(ttl: u64) {
    CACHE_TTL.set(ttl).ok();
}

#[allow(clippy::cast_possible_wrap)]
fn cache_ttl() -> i64 {
    *CACHE_TTL.get().unwrap_or(&300) as i64
}

fn cache_key(user_id: Uuid, project_id: Option<Uuid>) -> String {
    match project_id {
        Some(pid) => format!("perms:{user_id}:{pid}"),
        None => format!("perms:{user_id}:global"),
    }
}

/// Resolve all effective permissions for a user, optionally scoped to a project.
#[tracing::instrument(skip(pool, valkey), fields(%user_id), err)]
pub async fn effective_permissions(
    pool: &PgPool,
    valkey: &fred::clients::Pool,
    user_id: Uuid,
    project_id: Option<Uuid>,
) -> anyhow::Result<HashSet<Permission>> {
    let key = cache_key(user_id, project_id);

    if let Some(cached) = valkey::get_cached::<Vec<String>>(valkey, &key).await {
        let perms = cached
            .iter()
            .filter_map(|s| if let Ok(p) = Permission::from_str(s) {
                Some(p)
            } else {
                tracing::warn!(permission = %s, "unparseable permission string in cache, ignoring");
                None
            })
            .collect();
        return Ok(perms);
    }

    let perm_names: Vec<String> = sqlx::query_scalar!(
        r#"SELECT DISTINCT p.name as "name!"
        FROM permissions p
        WHERE p.id IN (
            SELECT rp.permission_id
            FROM user_roles ur
            JOIN role_permissions rp ON rp.role_id = ur.role_id
            WHERE ur.user_id = $1
              AND ur.project_id IS NULL

            UNION

            SELECT rp.permission_id
            FROM user_roles ur
            JOIN role_permissions rp ON rp.role_id = ur.role_id
            WHERE ur.user_id = $1
              AND ur.project_id = $2

            UNION

            SELECT d.permission_id
            FROM delegations d
            WHERE d.delegate_id = $1
              AND d.project_id IS NULL
              AND d.revoked_at IS NULL
              AND (d.expires_at IS NULL OR d.expires_at > now())

            UNION

            SELECT d.permission_id
            FROM delegations d
            WHERE d.delegate_id = $1
              AND d.project_id = $2
              AND d.revoked_at IS NULL
              AND (d.expires_at IS NULL OR d.expires_at > now())
        )"#,
        user_id,
        project_id,
    )
    .fetch_all(pool)
    .await?;

    let mut perms: HashSet<Permission> = perm_names
        .iter()
        .filter_map(|s| Permission::from_str(s).ok())
        .collect();

    if let Some(pid) = project_id {
        add_workspace_permissions(pool, &mut perms, user_id, pid).await?;
    }

    let cache_strings: Vec<String> = perms.iter().map(|p| p.as_str().to_owned()).collect();
    let _ = valkey::set_cached(valkey, &key, &cache_strings, cache_ttl()).await;

    Ok(perms)
}

/// Check whether a user has a specific permission.
#[tracing::instrument(skip(pool, valkey), fields(%user_id, %perm), err)]
pub async fn has_permission(
    pool: &PgPool,
    valkey: &fred::clients::Pool,
    user_id: Uuid,
    project_id: Option<Uuid>,
    perm: Permission,
) -> anyhow::Result<bool> {
    let perms = effective_permissions(pool, valkey, user_id, project_id).await?;
    Ok(perms.contains(&perm))
}

/// Check whether a user has a permission, intersected with optional API token scopes.
#[tracing::instrument(skip(pool, valkey), fields(%user_id, %perm), err)]
pub async fn has_permission_scoped(
    pool: &PgPool,
    valkey: &fred::clients::Pool,
    user_id: Uuid,
    project_id: Option<Uuid>,
    perm: Permission,
    token_scopes: Option<&[String]>,
) -> anyhow::Result<bool> {
    if !scope_allows(token_scopes, perm) {
        return Ok(false);
    }
    has_permission(pool, valkey, user_id, project_id, perm).await
}

/// Check whether a set of token scopes allows a given permission.
pub fn scope_allows(token_scopes: Option<&[String]>, perm: Permission) -> bool {
    let Some(scopes) = token_scopes else {
        return true;
    };
    if scopes.is_empty() || scopes.iter().any(|s| s == "*") {
        return true;
    }
    scopes.iter().any(|s| s == perm.as_str())
}

/// Grant implicit project permissions based on workspace membership.
async fn add_workspace_permissions(
    pool: &PgPool,
    perms: &mut HashSet<Permission>,
    user_id: Uuid,
    project_id: Uuid,
) -> anyhow::Result<()> {
    let role: Option<String> = sqlx::query_scalar!(
        r#"SELECT wm.role as "role!"
        FROM workspace_members wm
        JOIN projects p ON p.workspace_id = wm.workspace_id
        JOIN workspaces w ON w.id = wm.workspace_id
        WHERE p.id = $1 AND p.is_active = true AND w.is_active = true AND wm.user_id = $2"#,
        project_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?;

    if let Some(role) = role {
        perms.insert(Permission::ProjectRead);
        if role == "owner" || role == "admin" {
            perms.insert(Permission::ProjectWrite);
        }
    }

    Ok(())
}

/// Get permissions for a specific role by ID.
#[tracing::instrument(skip(pool), fields(%role_id), err)]
pub async fn role_permissions(pool: &PgPool, role_id: Uuid) -> anyhow::Result<HashSet<Permission>> {
    let names: Vec<String> = sqlx::query_scalar!(
        r#"SELECT p.name as "name!"
        FROM permissions p
        JOIN role_permissions rp ON rp.permission_id = p.id
        WHERE rp.role_id = $1"#,
        role_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(names
        .iter()
        .filter_map(|s| Permission::from_str(s).ok())
        .collect())
}

/// Invalidate cached permissions for a user.
#[tracing::instrument(skip(valkey), fields(%user_id), err)]
pub async fn invalidate_permissions(
    valkey: &fred::clients::Pool,
    user_id: Uuid,
    project_id: Option<Uuid>,
) -> anyhow::Result<()> {
    if let Some(pid) = project_id {
        valkey::invalidate(valkey, &cache_key(user_id, None)).await?;
        valkey::invalidate(valkey, &cache_key(user_id, Some(pid))).await?;
    } else {
        valkey::invalidate_pattern(valkey, &format!("perms:{user_id}:*")).await?;
    }
    Ok(())
}

/// Concrete [`PermissionChecker`] backed by Postgres + Valkey.
pub struct PgPermissionChecker<'a> {
    pub pool: &'a PgPool,
    pub valkey: &'a fred::clients::Pool,
}

impl PermissionChecker for PgPermissionChecker<'_> {
    async fn has_permission(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
        perm: Permission,
    ) -> anyhow::Result<bool> {
        has_permission(self.pool, self.valkey, user_id, project_id, perm).await
    }

    async fn has_permission_scoped(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
        perm: Permission,
        token_scopes: Option<&[String]>,
    ) -> anyhow::Result<bool> {
        has_permission_scoped(
            self.pool,
            self.valkey,
            user_id,
            project_id,
            perm,
            token_scopes,
        )
        .await
    }
}

impl PermissionResolver for PgPermissionChecker<'_> {
    async fn role_permissions(
        &self,
        role_id: Uuid,
    ) -> anyhow::Result<std::collections::HashSet<Permission>> {
        role_permissions(self.pool, role_id).await
    }

    async fn effective_permissions(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
    ) -> anyhow::Result<std::collections::HashSet<Permission>> {
        effective_permissions(self.pool, self.valkey, user_id, project_id).await
    }

    async fn invalidate_permissions(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        invalidate_permissions(self.valkey, user_id, project_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_with_project() {
        let user = Uuid::nil();
        let project = Uuid::max();
        let key = cache_key(user, Some(project));
        assert_eq!(key, format!("perms:{user}:{project}"));
    }

    #[test]
    fn cache_key_without_project() {
        let user = Uuid::nil();
        let key = cache_key(user, None);
        assert_eq!(key, format!("perms:{user}:global"));
    }

    #[test]
    fn scope_allows_none_is_unrestricted() {
        assert!(scope_allows(None, Permission::ProjectRead));
    }

    #[test]
    fn scope_allows_empty_is_unrestricted() {
        let scopes: Vec<String> = vec![];
        assert!(scope_allows(Some(&scopes), Permission::ProjectRead));
    }

    #[test]
    fn scope_allows_wildcard_is_unrestricted() {
        let scopes = vec!["*".to_string()];
        assert!(scope_allows(Some(&scopes), Permission::ProjectRead));
    }

    #[test]
    fn scope_allows_matching_permission() {
        let scopes = vec!["project:read".to_string(), "project:write".to_string()];
        assert!(scope_allows(Some(&scopes), Permission::ProjectRead));
        assert!(scope_allows(Some(&scopes), Permission::ProjectWrite));
    }

    #[test]
    fn scope_denies_non_matching_permission() {
        let scopes = vec!["project:read".to_string()];
        assert!(!scope_allows(Some(&scopes), Permission::ProjectWrite));
    }

    #[test]
    fn cache_ttl_defaults_to_300() {
        let ttl = cache_ttl();
        assert!(ttl > 0, "cache_ttl should be positive, got {ttl}");
    }
}
