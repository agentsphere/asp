// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Role-based access control and permission resolution.

pub mod delegation;
pub mod resolver;
pub mod types;

pub use types::Permission;
