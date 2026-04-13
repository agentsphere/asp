// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Shared types, error handling, and utility functions for the platform.

pub mod audit;
pub mod auth_user;
pub mod error;
pub mod events;
pub mod permission;
pub mod pool;
pub mod traits;
pub mod user_type;
pub mod validation;
pub mod valkey;

// Re-export key types at crate root for convenience.
pub use audit::{AuditEntry, AuditLog, send_audit};
pub use auth_user::{AuthUser, PermissionChecker, PermissionResolver, parse_user_type};
pub use error::ApiError;
pub use events::PlatformEvent;
pub use permission::Permission;
pub use traits::{
    AuditLogger, NotificationDispatcher, NotifyParams, SecretsResolver, TaskHeartbeat,
    WebhookDispatcher, WorkspaceMembershipChecker,
};
pub use user_type::UserType;
