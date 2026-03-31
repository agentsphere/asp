// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

pub mod control;
pub mod error;
pub mod messages;
pub mod session;
pub mod transport;

#[allow(unused_imports)] // Re-exported for test crate consumers
pub use error::CliError;
pub use messages::CliMessage;
#[allow(unused_imports)] // Re-exported for test crate consumers
pub use messages::CliUserInput;
pub use session::CliSessionManager;
#[allow(unused_imports)] // Re-exported for test crate consumers
pub use transport::{CliSpawnOptions, SubprocessTransport};
