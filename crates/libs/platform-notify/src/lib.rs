// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Notification dispatch: email (SMTP), in-app, webhook routing.
//!
//! Provides a concrete [`NotificationDispatcher`](platform_types::NotificationDispatcher)
//! implementation backed by Postgres, SMTP, and Valkey for rate limiting.

pub mod dispatch;
pub mod email;

pub use dispatch::SmtpNotificationDispatcher;
pub use email::SmtpConfig;
