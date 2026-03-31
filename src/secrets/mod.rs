// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Secrets engine: AES-256-GCM encryption, scoped access, and request flow.

#[allow(dead_code)]
pub mod engine;
pub mod llm_providers;
pub mod request;
pub mod user_keys;
