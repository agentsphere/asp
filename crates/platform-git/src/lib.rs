// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Git operations library for the platform.
//!
//! Provides trait-based abstractions over git CLI operations, plus pure-function
//! parsers for SSH keys, GPG keys, commit signatures, pkt-line protocol, and
//! SSH command parsing.
//!
//! ## Traits
//!
//! - [`GitRepo`] — read operations on bare repos (rev-parse, ls-tree, log, etc.)
//! - [`GitRepoManager`] — create/init repos, tags
//! - [`GitMerger`] — merge strategies via worktrees
//! - [`GitWorktreeWriter`] — write files via worktrees with per-repo locking
//! - [`PostReceiveHandler`] — side effects after push (app-specific, no default impl)
//! - [`BranchProtectionProvider`] — protection rule lookup (DB-backed, no default impl)
//! - [`GitAuthenticator`] — auth for git transports (DB-backed, no default impl)
//! - [`GitAccessControl`] — permission checks (DB-backed, no default impl)
//! - [`ProjectResolver`] — resolve owner/repo to project (DB-backed, no default impl)
//!
//! ## Concrete implementations
//!
//! - [`CliGitRepo`] — shells out to `git` CLI
//! - [`CliGitRepoManager`] — shells out to `git` plumbing commands
//! - [`CliGitMerger`] — merge via worktrees + `git` CLI
//! - [`CliGitWorktreeWriter`] — write files via worktrees + per-repo locking

pub mod browser_types;
pub mod error;
pub mod gpg_keys;
pub mod hooks;
pub mod lock;
pub mod ops;
pub mod plumbing;
pub mod protection;
pub mod signature;
pub mod ssh_command;
pub mod ssh_keys;
pub mod templates;
pub mod traits;
pub mod types;
pub mod validation;
pub mod worktree;

// Re-export key types at crate root for convenience.
pub use browser_types::{BlobContent, BranchInfo, CommitInfo, TreeEntry};
pub use error::{GitError, GpgKeyError, SshError, SshKeyError};
pub use hooks::{PostReceiveParams, RefUpdate};
pub use lock::RepoLock;
pub use ops::CliGitRepo;
pub use plumbing::CliGitRepoManager;
pub use protection::BranchProtection;
pub use signature::{SignatureInfo, SignatureStatus};
pub use ssh_command::ParsedCommand;
pub use ssh_keys::ParsedSshKey;
pub use templates::TemplateFile;
pub use traits::*;
pub use types::*;
pub use worktree::{CliGitMerger, CliGitWorktreeWriter};
