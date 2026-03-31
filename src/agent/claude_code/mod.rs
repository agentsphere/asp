// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

pub mod adapter;
pub mod pod;
#[allow(dead_code)] // Pending removal in Step 6 (dead code cleanup)
pub mod progress;

pub use adapter::ClaudeCodeProvider;
