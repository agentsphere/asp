// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Per-repo mutex for serializing git operations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use tokio::sync::Mutex;

/// Global map of per-repo mutexes. Ensures only one git worktree operation
/// runs on a given repo at a time (prevents concurrent worktree conflicts).
static REPO_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Acquire an exclusive lock for the given repository path.
///
/// Returns an owned guard that releases the lock on drop.
pub async fn repo_lock(repo_path: &std::path::Path) -> tokio::sync::OwnedMutexGuard<()> {
    let mut locks = REPO_LOCKS.lock().await;
    let lock = locks
        .entry(repo_path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    drop(locks); // Release the outer lock before awaiting the inner one
    lock.lock_owned().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repo_lock_same_path_serializes() {
        let path = PathBuf::from("/tmp/test-repo-lock-serialize");
        let guard1 = repo_lock(&path).await;
        // Try to acquire again — would deadlock if we didn't drop first
        drop(guard1);
        let guard2 = repo_lock(&path).await;
        drop(guard2);
    }

    #[tokio::test]
    async fn repo_lock_different_paths_independent() {
        let path1 = PathBuf::from("/tmp/test-repo-lock-a");
        let path2 = PathBuf::from("/tmp/test-repo-lock-b");
        let guard1 = repo_lock(&path1).await;
        let guard2 = repo_lock(&path2).await;
        drop(guard1);
        drop(guard2);
    }
}
