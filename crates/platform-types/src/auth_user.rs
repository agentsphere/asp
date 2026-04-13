// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::collections::HashSet;
use std::future::Future;

use uuid::Uuid;

use crate::error::ApiError;
use crate::permission::Permission;
use crate::user_type::UserType;

/// Authenticated user. Plain struct — no `FromRequestParts` impl.
/// Each binary implements its own axum extractor using the shared lookup functions.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub user_name: String,
    pub user_type: UserType,
    pub ip_addr: Option<String>,
    /// Token scopes if authenticated via API token.
    /// None = session auth (no scope restriction).
    pub token_scopes: Option<Vec<String>>,
    /// Hard workspace boundary from scoped API token.
    pub boundary_workspace_id: Option<Uuid>,
    /// Hard project boundary from scoped API token.
    pub boundary_project_id: Option<Uuid>,
    /// Agent session ID, extracted from token name `agent-session-{uuid}`.
    pub session_id: Option<Uuid>,
    /// SHA-256 hash of the session token when authenticated via session cookie/bearer.
    pub session_token_hash: Option<String>,
}

impl AuthUser {
    /// Record auth context fields into the current tracing span.
    pub fn record_to_span(&self) {
        let span = tracing::Span::current();
        span.record("user_id", tracing::field::display(self.user_id));
        span.record("user_type", tracing::field::display(&self.user_type));
        if let Some(sid) = &self.session_id {
            span.record("session_id", tracing::field::display(sid));
        }
    }

    /// Verify this request is allowed to access the given project.
    /// Returns 404 for scope violations (don't leak resource existence).
    pub fn check_project_scope(&self, project_id: Uuid) -> Result<(), ApiError> {
        if let Some(boundary_pid) = self.boundary_project_id
            && boundary_pid != project_id
        {
            return Err(ApiError::NotFound("project".into()));
        }
        Ok(())
    }

    /// Verify this request is allowed to access resources in the given workspace.
    /// Returns 404 for scope violations (don't leak resource existence).
    #[allow(dead_code)]
    pub fn check_workspace_scope(&self, workspace_id: Uuid) -> Result<(), ApiError> {
        if let Some(boundary_wid) = self.boundary_workspace_id
            && boundary_wid != workspace_id
        {
            return Err(ApiError::NotFound("workspace".into()));
        }
        Ok(())
    }
}

/// Parse `user_type` string from DB into the `UserType` enum.
pub fn parse_user_type(s: &str) -> Result<UserType, ApiError> {
    s.parse().map_err(|e: anyhow::Error| ApiError::Internal(e))
}

/// Trait for checking user permissions.
/// Decouples domain crates from the concrete RBAC implementation.
pub trait PermissionChecker: Send + Sync {
    fn has_permission(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
        perm: Permission,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;

    fn has_permission_scoped(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
        perm: Permission,
        token_scopes: Option<&[String]>,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
}

/// Trait for resolving permission sets.
///
/// Extends beyond `PermissionChecker` (single-permission boolean checks) to provide
/// full set resolution and cache management. Needed by agent identity (permission
/// intersection: `role_perms ∩ spawner_perms`) and admin delegation flows.
pub trait PermissionResolver: Send + Sync {
    /// Get all permissions assigned to a specific role.
    fn role_permissions(
        &self,
        role_id: Uuid,
    ) -> impl Future<Output = anyhow::Result<HashSet<Permission>>> + Send;

    /// Resolve all effective permissions for a user, optionally scoped to a project.
    /// Includes role-based, delegation-based, and workspace-derived permissions.
    fn effective_permissions(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
    ) -> impl Future<Output = anyhow::Result<HashSet<Permission>>> + Send;

    /// Invalidate cached permissions for a user.
    /// If `project_id` is `Some`, clears global + project-specific cache.
    /// If `None`, clears all cached permissions for this user.
    fn invalidate_permissions(
        &self,
        user_id: Uuid,
        project_id: Option<Uuid>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[cfg(test)]
impl AuthUser {
    pub fn test_human(user_id: Uuid) -> Self {
        Self {
            user_id,
            user_name: "test_user".into(),
            user_type: UserType::Human,
            ip_addr: Some("127.0.0.1".into()),
            token_scopes: None,
            boundary_workspace_id: None,
            boundary_project_id: None,
            session_id: None,
            session_token_hash: None,
        }
    }

    pub fn test_with_name(user_id: Uuid, name: &str) -> Self {
        Self {
            user_id,
            user_name: name.into(),
            user_type: UserType::Human,
            ip_addr: Some("127.0.0.1".into()),
            token_scopes: None,
            boundary_workspace_id: None,
            boundary_project_id: None,
            session_id: None,
            session_token_hash: None,
        }
    }

    pub fn test_with_scopes(user_id: Uuid, scopes: Vec<String>) -> Self {
        Self {
            user_id,
            user_name: "test_user".into(),
            user_type: UserType::Human,
            ip_addr: Some("127.0.0.1".into()),
            token_scopes: Some(scopes),
            boundary_workspace_id: None,
            boundary_project_id: None,
            session_id: None,
            session_token_hash: None,
        }
    }

    pub fn test_with_project_scope(user_id: Uuid, project_id: Uuid) -> Self {
        Self {
            user_id,
            user_name: "test_user".into(),
            user_type: UserType::Human,
            ip_addr: Some("127.0.0.1".into()),
            token_scopes: None,
            boundary_workspace_id: None,
            boundary_project_id: Some(project_id),
            session_id: None,
            session_token_hash: None,
        }
    }

    pub fn test_with_workspace_scope(user_id: Uuid, workspace_id: Uuid) -> Self {
        Self {
            user_id,
            user_name: "test_user".into(),
            user_type: UserType::Human,
            ip_addr: Some("127.0.0.1".into()),
            token_scopes: None,
            boundary_workspace_id: Some(workspace_id),
            boundary_project_id: None,
            session_id: None,
            session_token_hash: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_project_scope_none_allows_any() {
        let auth = AuthUser::test_human(Uuid::new_v4());
        assert!(auth.check_project_scope(Uuid::new_v4()).is_ok());
    }

    #[test]
    fn check_project_scope_matching_allows() {
        let project_id = Uuid::new_v4();
        let auth = AuthUser::test_with_project_scope(Uuid::new_v4(), project_id);
        assert!(auth.check_project_scope(project_id).is_ok());
    }

    #[test]
    fn check_project_scope_mismatch_returns_not_found() {
        let auth = AuthUser::test_with_project_scope(Uuid::new_v4(), Uuid::new_v4());
        let result = auth.check_project_scope(Uuid::new_v4());
        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[test]
    fn check_workspace_scope_none_allows_any() {
        let auth = AuthUser::test_human(Uuid::new_v4());
        assert!(auth.check_workspace_scope(Uuid::new_v4()).is_ok());
    }

    #[test]
    fn check_workspace_scope_matching_allows() {
        let ws_id = Uuid::new_v4();
        let auth = AuthUser::test_with_workspace_scope(Uuid::new_v4(), ws_id);
        assert!(auth.check_workspace_scope(ws_id).is_ok());
    }

    #[test]
    fn check_workspace_scope_mismatch_returns_not_found() {
        let auth = AuthUser::test_with_workspace_scope(Uuid::new_v4(), Uuid::new_v4());
        let result = auth.check_workspace_scope(Uuid::new_v4());
        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[test]
    fn parse_user_type_human() {
        let ut = parse_user_type("human").unwrap();
        assert_eq!(ut, UserType::Human);
    }

    #[test]
    fn parse_user_type_agent() {
        let ut = parse_user_type("agent").unwrap();
        assert_eq!(ut, UserType::Agent);
    }

    #[test]
    fn parse_user_type_unknown_returns_internal_error() {
        let err = parse_user_type("robot");
        assert!(matches!(err, Err(ApiError::Internal(_))));
    }

    #[test]
    fn record_to_span_without_session_id() {
        let auth = AuthUser::test_human(Uuid::new_v4());
        // Should not panic even though span fields may not be pre-defined
        auth.record_to_span();
    }

    #[test]
    fn record_to_span_with_session_id() {
        let mut auth = AuthUser::test_human(Uuid::new_v4());
        auth.session_id = Some(Uuid::new_v4());
        auth.record_to_span();
    }

    // Verify PermissionResolver trait is implementable with impl Future (no async-trait).
    struct MockPermissionResolver;
    impl PermissionResolver for MockPermissionResolver {
        async fn role_permissions(&self, _role_id: Uuid) -> anyhow::Result<HashSet<Permission>> {
            Ok(HashSet::from([Permission::ProjectRead]))
        }
        async fn effective_permissions(
            &self,
            _user_id: Uuid,
            _project_id: Option<Uuid>,
        ) -> anyhow::Result<HashSet<Permission>> {
            Ok(HashSet::from([
                Permission::ProjectRead,
                Permission::ProjectWrite,
            ]))
        }
        async fn invalidate_permissions(
            &self,
            _user_id: Uuid,
            _project_id: Option<Uuid>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_permission_resolver_role_permissions() {
        let resolver = MockPermissionResolver;
        let perms = resolver.role_permissions(Uuid::nil()).await.unwrap();
        assert!(perms.contains(&Permission::ProjectRead));
    }

    #[tokio::test]
    async fn mock_permission_resolver_effective_permissions() {
        let resolver = MockPermissionResolver;
        let perms = resolver
            .effective_permissions(Uuid::nil(), Some(Uuid::nil()))
            .await
            .unwrap();
        assert_eq!(perms.len(), 2);
        assert!(perms.contains(&Permission::ProjectRead));
        assert!(perms.contains(&Permission::ProjectWrite));
    }

    #[tokio::test]
    async fn mock_permission_resolver_invalidate() {
        let resolver = MockPermissionResolver;
        resolver
            .invalidate_permissions(Uuid::nil(), None)
            .await
            .unwrap();
    }
}
