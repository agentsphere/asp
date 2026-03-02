use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::templates;

/// Initialize a new bare git repository on disk with template files.
/// Returns the full path to the created repo directory.
///
/// Creates an initial commit containing platform template files
/// (`.platform.yaml`, `Dockerfile`, `deploy/production.yaml`, `CLAUDE.md`, `README.md`)
/// so the repo is immediately cloneable.
///
/// Called by the projects API when creating a project.
/// Does NOT update the `projects` table — that is the caller's responsibility.
#[tracing::instrument(skip(repos_path), fields(%owner, %name, %default_branch), err)]
pub async fn init_bare_repo(
    repos_path: &Path,
    owner: &str,
    name: &str,
    default_branch: &str,
) -> anyhow::Result<PathBuf> {
    let repo_dir = repos_path.join(owner).join(format!("{name}.git"));

    tokio::fs::create_dir_all(&repo_dir)
        .await
        .context("failed to create repo directory")?;

    let output = tokio::process::Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(&repo_dir)
        .output()
        .await
        .context("failed to run git init")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git init failed: {stderr}");
    }

    let head_ref = format!("ref: refs/heads/{default_branch}\n");
    tokio::fs::write(repo_dir.join("HEAD"), head_ref)
        .await
        .context("failed to set HEAD")?;

    let files = templates::project_template_files(name);
    create_initial_commit(&repo_dir, default_branch, &files)
        .await
        .context("failed to create initial commit")?;

    tracing::info!(path = %repo_dir.display(), "bare repository initialized with template");
    Ok(repo_dir)
}

/// Create the initial commit with template files in a bare repo using git plumbing.
async fn create_initial_commit(
    repo_dir: &Path,
    default_branch: &str,
    files: &[templates::TemplateFile],
) -> anyhow::Result<()> {
    let mut root_entries: Vec<String> = Vec::new();
    let mut subdirs: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();

    for file in files {
        if let Some((dir, filename)) = file.path.rsplit_once('/') {
            subdirs
                .entry(dir)
                .or_default()
                .push((filename, &file.content));
        } else {
            let blob = hash_object(repo_dir, &file.content).await?;
            root_entries.push(format!("100644 blob {blob}\t{}", file.path));
        }
    }

    for (dir_name, dir_files) in &subdirs {
        let mut sub_entries = Vec::new();
        for (filename, content) in dir_files {
            let blob = hash_object(repo_dir, content).await?;
            sub_entries.push(format!("100644 blob {blob}\t{filename}"));
        }
        let subtree = mktree(repo_dir, &sub_entries).await?;
        root_entries.push(format!("040000 tree {subtree}\t{dir_name}"));
    }

    let root_tree = mktree(repo_dir, &root_entries).await?;
    let commit = commit_tree(repo_dir, &root_tree, "Initial commit: platform template").await?;
    update_ref(repo_dir, default_branch, &commit).await
}

async fn hash_object(repo_dir: &Path, content: &str) -> anyhow::Result<String> {
    let mut child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn git hash-object")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(content.as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git hash-object failed: {stderr}");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

async fn mktree(repo_dir: &Path, entries: &[String]) -> anyhow::Result<String> {
    let input = entries.join("\n") + "\n";
    let mut child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .arg("mktree")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn git mktree")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(input.as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git mktree failed: {stderr}");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

async fn commit_tree(repo_dir: &Path, tree_hash: &str, message: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["commit-tree", tree_hash, "-m", message])
        .env("GIT_AUTHOR_NAME", "Platform")
        .env("GIT_AUTHOR_EMAIL", "platform@localhost")
        .env("GIT_COMMITTER_NAME", "Platform")
        .env("GIT_COMMITTER_EMAIL", "platform@localhost")
        .output()
        .await
        .context("failed to run git commit-tree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit-tree failed: {stderr}");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

async fn update_ref(repo_dir: &Path, branch: &str, commit_hash: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["update-ref", &format!("refs/heads/{branch}"), commit_hash])
        .output()
        .await
        .context("failed to run git update-ref")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git update-ref failed: {stderr}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_bare_repo_creates_directory() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", uuid::Uuid::new_v4()));
        let path = init_bare_repo(&tmp, "alice", "my-project", "main")
            .await
            .unwrap();

        assert!(path.exists());
        assert!(path.join("HEAD").exists());

        let head = tokio::fs::read_to_string(path.join("HEAD")).await.unwrap();
        assert_eq!(head, "ref: refs/heads/main\n");

        // Verify initial commit exists on main
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "refs/heads/main"])
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "main branch should have a commit");

        // Verify template files are in the tree
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["ls-tree", "--name-only", "main"])
            .output()
            .await
            .unwrap();
        let files = String::from_utf8(output.stdout).unwrap();
        assert!(files.contains(".platform.yaml"));
        assert!(files.contains("Dockerfile"));
        assert!(files.contains("CLAUDE.md"));
        assert!(files.contains("README.md"));
        assert!(files.contains("deploy"));

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn init_bare_repo_custom_branch() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", uuid::Uuid::new_v4()));
        let path = init_bare_repo(&tmp, "bob", "repo", "develop")
            .await
            .unwrap();

        let head = tokio::fs::read_to_string(path.join("HEAD")).await.unwrap();
        assert_eq!(head, "ref: refs/heads/develop\n");

        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "refs/heads/develop"])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "develop branch should have a commit"
        );

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn init_bare_repo_readme_contains_project_name() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", uuid::Uuid::new_v4()));
        let path = init_bare_repo(&tmp, "alice", "my-app", "main")
            .await
            .unwrap();

        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["show", "main:README.md"])
            .output()
            .await
            .unwrap();
        let content = String::from_utf8(output.stdout).unwrap();
        assert!(
            content.contains("my-app"),
            "README should contain project name"
        );

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn init_bare_repo_has_nested_deploy_directory() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", uuid::Uuid::new_v4()));
        let path = init_bare_repo(&tmp, "alice", "my-app", "main")
            .await
            .unwrap();

        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["show", "main:deploy/production.yaml"])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "deploy/production.yaml should exist"
        );
        let content = String::from_utf8(output.stdout).unwrap();
        assert!(
            content.contains("kind: Deployment"),
            "should contain K8s Deployment"
        );

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn init_bare_repo_commit_message() {
        let tmp = std::env::temp_dir().join(format!("platform-test-{}", uuid::Uuid::new_v4()));
        let path = init_bare_repo(&tmp, "alice", "my-app", "main")
            .await
            .unwrap();

        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["log", "--format=%s", "-1", "main"])
            .output()
            .await
            .unwrap();
        let message = String::from_utf8(output.stdout).unwrap();
        assert_eq!(message.trim(), "Initial commit: platform template");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
