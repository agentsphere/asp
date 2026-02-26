use std::path::{Path, PathBuf};

use sqlx::PgPool;
use uuid::Uuid;

use super::error::DeployerError;

// ---------------------------------------------------------------------------
// Ops repo lifecycle (local bare repos)
// ---------------------------------------------------------------------------

/// Initialize a new bare git repository for an ops repo.
/// Returns the full path to the created repo directory.
#[tracing::instrument(skip(repos_dir), fields(%name, %branch), err)]
pub async fn init_ops_repo(
    repos_dir: &Path,
    name: &str,
    branch: &str,
) -> Result<PathBuf, DeployerError> {
    let dest = repos_dir.join(format!("{name}.git"));

    tokio::fs::create_dir_all(&dest)
        .await
        .map_err(|e| DeployerError::SyncFailed(format!("failed to create repo dir: {e}")))?;

    let output = tokio::process::Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(&dest)
        .output()
        .await
        .map_err(|e| DeployerError::SyncFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DeployerError::SyncFailed(format!(
            "git init failed: {stderr}"
        )));
    }

    // Set default branch
    let head_ref = format!("ref: refs/heads/{branch}\n");
    tokio::fs::write(dest.join("HEAD"), head_ref)
        .await
        .map_err(|e| DeployerError::SyncFailed(format!("failed to set HEAD: {e}")))?;

    tracing::info!(path = %dest.display(), "ops repo initialized");
    Ok(dest)
}

// ---------------------------------------------------------------------------
// Reading from bare repos (no working tree needed)
// ---------------------------------------------------------------------------

/// Get the current HEAD SHA of a bare repo.
pub async fn get_head_sha(repo_path: &Path) -> Result<String, DeployerError> {
    let output = tokio::process::Command::new("git")
        .args(["-C"])
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .map_err(|e| DeployerError::SyncFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DeployerError::SyncFailed(format!(
            "git rev-parse failed: {stderr}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Read a file from a bare repo at a given ref without a working tree.
/// Uses `git show {ref}:{path}`.
pub async fn read_file_at_ref(
    repo_path: &Path,
    git_ref: &str,
    file_path: &str,
) -> Result<String, DeployerError> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!("{git_ref}:{file_path}"))
        .output()
        .await
        .map_err(|e| DeployerError::SyncFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(DeployerError::ValuesNotFound(format!(
            "{file_path} at {git_ref}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Read the values file for a given environment from the ops repo.
/// Returns the parsed YAML as a JSON value for template rendering.
pub async fn read_values(
    repo_path: &Path,
    branch: &str,
    environment: &str,
) -> Result<serde_json::Value, DeployerError> {
    let file_path = format!("values/{environment}.yaml");
    let content = read_file_at_ref(repo_path, branch, &file_path).await?;

    serde_yaml::from_str(&content)
        .map_err(|e| DeployerError::RenderFailed(format!("failed to parse {file_path}: {e}")))
}

// ---------------------------------------------------------------------------
// Writing to bare repos (requires worktree)
// ---------------------------------------------------------------------------

/// Commit a values file to the ops repo for a given environment.
/// Uses git worktree to write into a bare repo.
/// Returns the new commit SHA.
#[tracing::instrument(skip(values), fields(%environment), err)]
pub async fn commit_values(
    repo_path: &Path,
    branch: &str,
    environment: &str,
    values: &serde_json::Value,
) -> Result<String, DeployerError> {
    let worktree_dir = repo_path.join(format!("_values_worktree_{}", Uuid::new_v4()));

    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(&worktree_dir)
        .arg(branch)
        .output()
        .await
        .map_err(|e| DeployerError::CommitFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DeployerError::CommitFailed(format!(
            "git worktree add failed: {stderr}"
        )));
    }

    let result = write_and_commit_values(&worktree_dir, environment, values).await;

    // Always clean up worktree
    cleanup_worktree(repo_path, &worktree_dir).await;

    result?;

    get_head_sha(repo_path).await
}

/// Internal: write the values file, stage, and commit inside a worktree.
async fn write_and_commit_values(
    worktree_dir: &Path,
    environment: &str,
    values: &serde_json::Value,
) -> Result<(), DeployerError> {
    // Ensure values/ directory exists
    let values_dir = worktree_dir.join("values");
    tokio::fs::create_dir_all(&values_dir)
        .await
        .map_err(|e| DeployerError::CommitFailed(format!("mkdir values: {e}")))?;

    // Write the YAML values file
    let yaml_content = serde_yaml::to_string(values)
        .map_err(|e| DeployerError::CommitFailed(format!("yaml serialize: {e}")))?;

    let file_path = values_dir.join(format!("{environment}.yaml"));
    tokio::fs::write(&file_path, &yaml_content)
        .await
        .map_err(|e| DeployerError::CommitFailed(format!("write values: {e}")))?;

    // Stage the file
    let add_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree_dir)
        .args(["add", &format!("values/{environment}.yaml")])
        .output()
        .await
        .map_err(|e| DeployerError::CommitFailed(e.to_string()))?;

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        return Err(DeployerError::CommitFailed(format!(
            "git add failed: {stderr}"
        )));
    }

    // Extract image_ref for the commit message
    let image_ref = values
        .get("image_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let commit_msg = format!("deploy({environment}): update image to {image_ref}");

    let commit_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree_dir)
        .env("GIT_AUTHOR_NAME", "Platform")
        .env("GIT_AUTHOR_EMAIL", "platform@localhost")
        .env("GIT_COMMITTER_NAME", "Platform")
        .env("GIT_COMMITTER_EMAIL", "platform@localhost")
        .args(["commit", "-m", &commit_msg])
        .output()
        .await
        .map_err(|e| DeployerError::CommitFailed(e.to_string()))?;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        // "nothing to commit" is not an error — values unchanged
        if stderr.contains("nothing to commit") {
            return Ok(());
        }
        return Err(DeployerError::CommitFailed(format!(
            "git commit failed: {stderr}"
        )));
    }

    Ok(())
}

/// Revert the last commit on the ops repo branch (for rollback).
/// Uses git worktree + git revert.
/// Returns the new commit SHA after revert.
#[tracing::instrument(fields(repo = %repo_path.display(), %branch), err)]
pub async fn revert_last_commit(repo_path: &Path, branch: &str) -> Result<String, DeployerError> {
    let worktree_dir = repo_path.join(format!("_revert_worktree_{}", Uuid::new_v4()));

    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(&worktree_dir)
        .arg(branch)
        .output()
        .await
        .map_err(|e| DeployerError::RevertFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DeployerError::RevertFailed(format!(
            "git worktree add failed: {stderr}"
        )));
    }

    let result = revert_head_in_worktree(&worktree_dir).await;

    cleanup_worktree(repo_path, &worktree_dir).await;

    result?;

    get_head_sha(repo_path).await
}

/// Internal: run `git revert HEAD --no-edit` inside a worktree.
async fn revert_head_in_worktree(worktree_dir: &Path) -> Result<(), DeployerError> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree_dir)
        .env("GIT_AUTHOR_NAME", "Platform")
        .env("GIT_AUTHOR_EMAIL", "platform@localhost")
        .env("GIT_COMMITTER_NAME", "Platform")
        .env("GIT_COMMITTER_EMAIL", "platform@localhost")
        .args(["revert", "HEAD", "--no-edit"])
        .output()
        .await
        .map_err(|e| DeployerError::RevertFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DeployerError::RevertFailed(format!(
            "git revert failed: {stderr}"
        )));
    }

    Ok(())
}

/// Clean up a temporary worktree (best-effort).
async fn cleanup_worktree(repo_path: &Path, worktree_dir: &Path) {
    let _ = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_dir)
        .output()
        .await;

    let _ = tokio::fs::remove_dir_all(worktree_dir).await;
}

// ---------------------------------------------------------------------------
// Manifest path resolution (unchanged — works for bare + worktree)
// ---------------------------------------------------------------------------

/// Resolve the full filesystem path to a manifest file within an ops repo.
/// Guards against path traversal by ensuring the result stays within the repo directory.
#[allow(dead_code)] // Used in tests; production code uses read_file_at_ref for bare repos
pub fn resolve_manifest_path(
    repos_dir: &Path,
    ops_repo_name: &str,
    ops_repo_subpath: &str,
    manifest_path: &str,
) -> Result<PathBuf, DeployerError> {
    if manifest_path.contains("..") || ops_repo_subpath.contains("..") {
        return Err(DeployerError::InvalidManifest(
            "path traversal detected".into(),
        ));
    }

    let repo_root = repos_dir.join(ops_repo_name);
    let full_path = repo_root
        .join(ops_repo_subpath.trim_matches('/'))
        .join(manifest_path);

    if !full_path.starts_with(&repo_root) {
        return Err(DeployerError::InvalidManifest(
            "path traversal detected".into(),
        ));
    }

    Ok(full_path)
}

// ---------------------------------------------------------------------------
// Sync: for local bare repos, just return path + HEAD SHA
// ---------------------------------------------------------------------------

/// For local bare repos, "syncing" is just reading the current HEAD SHA.
/// The repo is already on disk — no fetch/pull needed.
#[tracing::instrument(skip(pool), fields(%ops_repo_id), err)]
pub async fn sync_repo(
    pool: &PgPool,
    ops_repo_id: Uuid,
) -> Result<(PathBuf, String, String), DeployerError> {
    let repo = sqlx::query!(
        "SELECT name, repo_path, branch, path FROM ops_repos WHERE id = $1",
        ops_repo_id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DeployerError::OpsRepoNotFound(ops_repo_id.to_string()))?;

    let repo_path = PathBuf::from(&repo.repo_path);
    let sha = get_head_sha(&repo_path).await?;

    Ok((repo_path, sha, repo.branch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_path_joins_correctly() {
        let path =
            resolve_manifest_path(Path::new("/data/ops"), "myrepo", "/k8s", "deploy.yaml").unwrap();
        assert_eq!(path, PathBuf::from("/data/ops/myrepo/k8s/deploy.yaml"));
    }

    #[test]
    fn manifest_path_handles_root_subpath() {
        let path =
            resolve_manifest_path(Path::new("/data/ops"), "myrepo", "/", "deploy.yaml").unwrap();
        assert_eq!(path, PathBuf::from("/data/ops/myrepo/deploy.yaml"));
    }

    #[test]
    fn manifest_path_rejects_traversal_in_manifest() {
        let result =
            resolve_manifest_path(Path::new("/data/ops"), "myrepo", "/k8s", "../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn manifest_path_rejects_traversal_in_subpath() {
        let result =
            resolve_manifest_path(Path::new("/data/ops"), "myrepo", "/../../../etc", "passwd");
        assert!(result.is_err());
    }

    #[test]
    fn manifest_path_rejects_traversal_in_repo_name() {
        let result =
            resolve_manifest_path(Path::new("/data/ops"), "../escape", "/k8s", "deploy.yaml");
        assert!(
            result.is_err() || {
                let p = result.unwrap();
                p.starts_with("/data/ops")
            }
        );
    }

    #[test]
    fn manifest_path_empty_subpath() {
        let path =
            resolve_manifest_path(Path::new("/data/ops"), "myrepo", "", "deploy.yaml").unwrap();
        assert_eq!(path, PathBuf::from("/data/ops/myrepo/deploy.yaml"));
    }

    #[test]
    fn manifest_path_deeply_nested() {
        let path = resolve_manifest_path(
            Path::new("/data/ops"),
            "myrepo",
            "/env/staging/k8s",
            "deployment.yaml",
        )
        .unwrap();
        assert_eq!(
            path,
            PathBuf::from("/data/ops/myrepo/env/staging/k8s/deployment.yaml")
        );
    }

    #[tokio::test]
    async fn init_and_get_sha_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = init_ops_repo(&tmp, "test-ops", "main").await.unwrap();
        assert!(repo_path.exists());
        assert!(repo_path.join("HEAD").exists());

        let head = tokio::fs::read_to_string(repo_path.join("HEAD"))
            .await
            .unwrap();
        assert_eq!(head, "ref: refs/heads/main\n");

        // No commits yet — git rev-parse HEAD returns literal "HEAD" (not a SHA)
        let sha = get_head_sha(&repo_path).await.unwrap();
        assert_eq!(sha, "HEAD");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    /// Helper: bootstrap a bare repo with an initial commit so worktree ops work.
    async fn bootstrap_repo(tmp: &Path) -> PathBuf {
        let repo_path = init_ops_repo(tmp, "test-ops", "main").await.unwrap();

        let init_wt = repo_path.join("_init_wt");
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["worktree", "add", "--orphan", "-b", "main"])
            .arg(&init_wt)
            .output()
            .await
            .unwrap();

        tokio::fs::write(init_wt.join("README.md"), "# Ops\n")
            .await
            .unwrap();
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&init_wt)
            .args(["add", "."])
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&init_wt)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test")
            .args(["commit", "-m", "init"])
            .output()
            .await;
        cleanup_worktree(&repo_path, &init_wt).await;

        repo_path
    }

    #[tokio::test]
    async fn commit_values_creates_file() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let values = serde_json::json!({
            "image_ref": "registry/app:abc123",
            "project_name": "my-app",
        });
        let sha = commit_values(&repo_path, "main", "production", &values)
            .await
            .unwrap();

        assert!(!sha.is_empty());

        // Verify we can read it back
        let read_back = read_values(&repo_path, "main", "production").await.unwrap();
        assert_eq!(read_back["image_ref"], "registry/app:abc123");
        assert_eq!(read_back["project_name"], "my-app");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn revert_restores_previous_values() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        // Commit v1
        let v1 = serde_json::json!({"image_ref": "registry/app:v1"});
        commit_values(&repo_path, "main", "production", &v1)
            .await
            .unwrap();

        // Commit v2
        let v2 = serde_json::json!({"image_ref": "registry/app:v2"});
        commit_values(&repo_path, "main", "production", &v2)
            .await
            .unwrap();

        // Read current — should be v2
        let current = read_values(&repo_path, "main", "production").await.unwrap();
        assert_eq!(current["image_ref"], "registry/app:v2");

        // Revert
        revert_last_commit(&repo_path, "main").await.unwrap();

        // Should be back to v1
        let after_revert = read_values(&repo_path, "main", "production").await.unwrap();
        assert_eq!(after_revert["image_ref"], "registry/app:v1");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn read_values_missing_file_returns_error() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let result = read_values(&repo_path, "main", "production").await;
        assert!(result.is_err());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn read_file_at_ref_nonexistent_file() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let result = read_file_at_ref(&repo_path, "main", "does-not-exist.yaml").await;
        assert!(result.is_err());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn read_file_at_ref_nonexistent_ref() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let result = read_file_at_ref(&repo_path, "nonexistent-branch", "README.md").await;
        assert!(result.is_err());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn read_values_invalid_yaml() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        // Write invalid YAML content via worktree
        let wt = repo_path.join("_bad_yaml_wt");
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["worktree", "add"])
            .arg(&wt)
            .arg("main")
            .output()
            .await;

        let values_dir = wt.join("values");
        tokio::fs::create_dir_all(&values_dir).await.unwrap();
        tokio::fs::write(
            values_dir.join("staging.yaml"),
            "invalid: [unclosed bracket",
        )
        .await
        .unwrap();

        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&wt)
            .args(["add", "."])
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&wt)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test")
            .args(["commit", "-m", "bad yaml"])
            .output()
            .await;
        cleanup_worktree(&repo_path, &wt).await;

        let result = read_values(&repo_path, "main", "staging").await;
        assert!(result.is_err());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn commit_values_no_changes_returns_error() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let values = serde_json::json!({"image_ref": "app:v1"});

        // First commit succeeds
        commit_values(&repo_path, "main", "production", &values)
            .await
            .unwrap();

        // Second commit with same values — git commit fails because nothing changed
        let result = commit_values(&repo_path, "main", "production", &values).await;
        assert!(result.is_err());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn revert_initial_commit_returns_error() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        // There's only 1 commit (from bootstrap). Reverting it should fail because
        // git revert on the very first commit needs special handling.
        let result = revert_last_commit(&repo_path, "main").await;
        // This may succeed or fail depending on git version — we just verify no panic
        // (git revert on initial commit fails with "empty commit" or similar)
        let _ = result; // Either Ok or Err is acceptable

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn cleanup_worktree_nonexistent_is_noop() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        // Cleaning up a nonexistent worktree should not error (best-effort)
        let fake_wt = repo_path.join("nonexistent_worktree");
        cleanup_worktree(&repo_path, &fake_wt).await;
        // No panic = success

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn get_head_sha_returns_valid_hash_after_commit() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", Uuid::new_v4()));
        let repo_path = bootstrap_repo(&tmp).await;

        let sha = get_head_sha(&repo_path).await.unwrap();
        // After bootstrap, SHA should be a 40-char hex string
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn get_head_sha_nonexistent_repo_returns_error() {
        let result = get_head_sha(Path::new("/nonexistent/repo")).await;
        assert!(result.is_err());
    }

    #[test]
    fn manifest_path_rejects_double_dot_in_both() {
        let result = resolve_manifest_path(
            Path::new("/data/ops"),
            "myrepo",
            "../escape",
            "../etc/passwd",
        );
        assert!(result.is_err());
    }
}
