// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Webhook dispatch with HMAC-SHA256 signing and SSRF protection.
//!
//! Provides a concrete [`WebhookDispatcher`](platform_types::WebhookDispatcher)
//! implementation backed by Postgres + reqwest.

pub mod dispatch;
pub mod signing;
pub mod ssrf;

pub use dispatch::WebhookDispatch;
pub use signing::sign_payload;
pub use ssrf::{SsrfError, validate_webhook_url};
