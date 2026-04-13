// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Cross-module trait contracts.
//!
//! These traits define the communication boundaries between domain crates.
//! Domain crates depend on these traits (via `platform-types`), not on each
//! other. Concrete implementations live in their respective modules or `src/`.
//!
//! Uses `impl Future` return types (stable Rust 1.75+, edition 2024) — no
//! `async-trait` crate needed. Works with generics (`&impl Trait`), not
//! `dyn` dispatch.

use std::future::Future;

use uuid::Uuid;

use crate::audit::AuditEntry;

// Note: `PermissionChecker` is defined in `auth_user.rs` alongside `AuthUser`
// because it's tightly coupled to the auth user context. Re-exported from
// `crate::auth_user::PermissionChecker`.

/// Trait for fire-and-forget audit logging.
///
/// Decouples domain crates from the concrete `AuditLog` implementation
/// (which requires a `PgPool` and `tokio::spawn`).
pub trait AuditLogger: Send + Sync {
    fn send_audit(&self, entry: AuditEntry);
}

/// Trait for resolving decrypted secrets.
///
/// Decouples domain crates (pipeline executor, agent identity, deployer)
/// from the concrete secrets engine implementation.
pub trait SecretsResolver: Send + Sync {
    /// Resolve a secret by name for a project, enforcing scope.
    fn resolve_secret(
        &self,
        project_id: Uuid,
        name: &str,
        requested_scope: &str,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;

    /// Resolve a secret using the full hierarchy (project+env > project > workspace > global).
    fn resolve_secret_hierarchical(
        &self,
        project_id: Uuid,
        workspace_id: Option<Uuid>,
        environment: Option<&str>,
        name: &str,
        requested_scope: &str,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;

    /// Replace `${{ secrets.NAME }}` patterns in a template string.
    fn resolve_secrets_for_env(
        &self,
        project_id: Uuid,
        scope: &str,
        template: &str,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;
}

/// Parameters for dispatching a notification.
pub struct NotifyParams<'a> {
    pub user_id: Uuid,
    pub notification_type: &'a str,
    pub subject: &'a str,
    pub body: Option<&'a str>,
    pub channel: &'a str,
    pub ref_type: Option<&'a str>,
    pub ref_id: Option<Uuid>,
}

/// Trait for dispatching notifications (email, in-app, webhook).
///
/// Decouples alert evaluation and other event producers from the concrete
/// notification dispatch implementation.
pub trait NotificationDispatcher: Send + Sync {
    /// Send a notification to a user.
    fn notify(&self, params: NotifyParams<'_>) -> impl Future<Output = anyhow::Result<()>> + Send;
}

/// Trait for checking workspace membership.
///
/// Decouples domain crates (registry access control) from the concrete
/// workspace service implementation.
pub trait WorkspaceMembershipChecker: Send + Sync {
    fn is_member(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
}

/// Trait for dispatching webhooks to external URLs.
///
/// Decouples domain crates from the concrete webhook dispatch implementation
/// (HTTP client, HMAC signing, concurrency control).
pub trait WebhookDispatcher: Send + Sync {
    /// Fire webhooks for a project event.
    fn fire_webhooks(
        &self,
        project_id: Uuid,
        event_name: &str,
        payload: &serde_json::Value,
    ) -> impl Future<Output = ()> + Send;
}

/// Trait for background task heartbeat tracking.
///
/// Decouples domain crates (agent reaper, pipeline executor) from the
/// concrete `TaskRegistry` implementation in `src/health/`.
pub trait TaskHeartbeat: Send + Sync {
    /// Register a task with its expected heartbeat interval.
    fn register(&self, name: &str, expected_interval_secs: u64);
    /// Record a successful heartbeat for a named task.
    fn heartbeat(&self, name: &str);
    /// Record an error for a named task.
    fn report_error(&self, name: &str, message: &str);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditEntry;

    // Verify traits are object-safe enough for our use case (impl Future, not dyn)
    struct MockAuditLogger;
    impl AuditLogger for MockAuditLogger {
        fn send_audit(&self, _entry: AuditEntry) {}
    }

    struct MockSecretsResolver;
    impl SecretsResolver for MockSecretsResolver {
        async fn resolve_secret(
            &self,
            _project_id: Uuid,
            name: &str,
            _scope: &str,
        ) -> anyhow::Result<String> {
            Ok(format!("mock-{name}"))
        }
        async fn resolve_secret_hierarchical(
            &self,
            _project_id: Uuid,
            _workspace_id: Option<Uuid>,
            _env: Option<&str>,
            name: &str,
            _scope: &str,
        ) -> anyhow::Result<String> {
            Ok(format!("mock-hier-{name}"))
        }
        async fn resolve_secrets_for_env(
            &self,
            _project_id: Uuid,
            _scope: &str,
            template: &str,
        ) -> anyhow::Result<String> {
            Ok(template.to_string())
        }
    }

    struct MockNotificationDispatcher;
    impl NotificationDispatcher for MockNotificationDispatcher {
        async fn notify(&self, _params: NotifyParams<'_>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockWebhookDispatcher;
    impl WebhookDispatcher for MockWebhookDispatcher {
        async fn fire_webhooks(
            &self,
            _project_id: Uuid,
            _event_name: &str,
            _payload: &serde_json::Value,
        ) {
        }
    }

    struct MockWorkspaceMembershipChecker;
    impl WorkspaceMembershipChecker for MockWorkspaceMembershipChecker {
        async fn is_member(&self, _workspace_id: Uuid, _user_id: Uuid) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    struct MockTaskHeartbeat;
    impl TaskHeartbeat for MockTaskHeartbeat {
        fn register(&self, _name: &str, _expected_interval_secs: u64) {}
        fn heartbeat(&self, _name: &str) {}
        fn report_error(&self, _name: &str, _message: &str) {}
    }

    #[test]
    fn mock_audit_logger_works() {
        let logger = MockAuditLogger;
        logger.send_audit(AuditEntry {
            actor_id: Uuid::nil(),
            actor_name: "test".into(),
            action: "test.action".into(),
            resource: "test".into(),
            resource_id: None,
            project_id: None,
            detail: None,
            ip_addr: None,
        });
    }

    #[tokio::test]
    async fn mock_secrets_resolver_works() {
        let resolver = MockSecretsResolver;
        let val = resolver
            .resolve_secret(Uuid::nil(), "DB_URL", "pipeline")
            .await
            .unwrap();
        assert_eq!(val, "mock-DB_URL");
    }

    #[tokio::test]
    async fn mock_notification_dispatcher_works() {
        let dispatcher = MockNotificationDispatcher;
        dispatcher
            .notify(NotifyParams {
                user_id: Uuid::nil(),
                notification_type: "test",
                subject: "subject",
                body: None,
                channel: "in_app",
                ref_type: None,
                ref_id: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn mock_webhook_dispatcher_works() {
        let dispatcher = MockWebhookDispatcher;
        dispatcher
            .fire_webhooks(Uuid::nil(), "push", &serde_json::json!({}))
            .await;
    }

    #[tokio::test]
    async fn mock_secrets_resolver_hierarchical() {
        let resolver = MockSecretsResolver;
        let val = resolver
            .resolve_secret_hierarchical(
                Uuid::nil(),
                Some(Uuid::nil()),
                Some("production"),
                "API_KEY",
                "pipeline",
            )
            .await
            .unwrap();
        assert_eq!(val, "mock-hier-API_KEY");
    }

    #[tokio::test]
    async fn mock_secrets_resolver_for_env() {
        let resolver = MockSecretsResolver;
        let val = resolver
            .resolve_secrets_for_env(Uuid::nil(), "pipeline", "host=${{ secrets.HOST }}")
            .await
            .unwrap();
        assert_eq!(val, "host=${{ secrets.HOST }}");
    }

    #[tokio::test]
    async fn mock_workspace_membership_checker_works() {
        let checker = MockWorkspaceMembershipChecker;
        let is_member = checker.is_member(Uuid::nil(), Uuid::nil()).await.unwrap();
        assert!(is_member);
    }

    #[test]
    fn mock_task_heartbeat_works() {
        let tracker = MockTaskHeartbeat;
        tracker.register("test-task", 60);
        tracker.heartbeat("test-task");
        tracker.report_error("test-task", "connection refused");
    }
}
