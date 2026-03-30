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

/// Initialize a bare repo with custom template files (instead of the default project templates).
/// Used by demo project creation to provide demo-specific files.
#[tracing::instrument(skip(repos_path, files), fields(%owner, %name, %default_branch), err)]
pub async fn init_bare_repo_with_files(
    repos_path: &Path,
    owner: &str,
    name: &str,
    default_branch: &str,
    files: &[templates::TemplateFile],
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

    create_initial_commit(&repo_dir, default_branch, files)
        .await
        .context("failed to create initial commit")?;

    tracing::info!(path = %repo_dir.display(), "bare repository initialized with custom files");
    Ok(repo_dir)
}

/// Create the initial commit with template files in a bare repo using git plumbing.
///
/// Supports arbitrarily nested paths (e.g. `.claude/commands/dev.md`) by
/// building git trees bottom-up.
async fn create_initial_commit(
    repo_dir: &Path,
    default_branch: &str,
    files: &[templates::TemplateFile],
) -> anyhow::Result<()> {
    // Build a nested map: path segments → content.
    // Each node is either a blob (file) or a tree (directory).
    let mut tree = DirNode::default();
    for file in files {
        tree.insert(file.path, &file.content);
    }

    let root_hash = tree.write_tree(repo_dir).await?;
    let commit = commit_tree(repo_dir, &root_hash, "Initial commit: platform template").await?;
    update_ref(repo_dir, default_branch, &commit).await
}

/// A directory node in the tree being built for the initial commit.
#[derive(Default)]
struct DirNode<'a> {
    /// Files directly in this directory: (filename, content).
    files: Vec<(&'a str, &'a str)>,
    /// Subdirectories: name → node.
    dirs: BTreeMap<&'a str, DirNode<'a>>,
}

impl<'a> DirNode<'a> {
    /// Insert a file at the given slash-separated path.
    fn insert(&mut self, path: &'a str, content: &'a str) {
        if let Some((first, rest)) = path.split_once('/') {
            self.dirs.entry(first).or_default().insert(rest, content);
        } else {
            self.files.push((path, content));
        }
    }

    /// Recursively write this directory as a git tree object, returning the tree hash.
    fn write_tree<'b>(
        &'b self,
        repo_dir: &'b Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + 'b>>
    {
        Box::pin(async move {
            let mut entries = Vec::new();

            for (filename, content) in &self.files {
                let blob = hash_object(repo_dir, content).await?;
                entries.push(format!("100644 blob {blob}\t{filename}"));
            }

            for (dir_name, child) in &self.dirs {
                let subtree = child.write_tree(repo_dir).await?;
                entries.push(format!("040000 tree {subtree}\t{dir_name}"));
            }

            mktree(repo_dir, &entries).await
        })
    }
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

    #[test]
    fn dir_node_insert_simple_file() {
        let mut node = DirNode::default();
        node.insert("README.md", "# Hello");
        assert_eq!(node.files.len(), 1);
        assert_eq!(node.files[0].0, "README.md");
        assert_eq!(node.files[0].1, "# Hello");
        assert!(node.dirs.is_empty());
    }

    #[test]
    fn dir_node_insert_nested_file() {
        let mut node = DirNode::default();
        node.insert("src/main.rs", "fn main() {}");
        assert!(node.files.is_empty());
        assert_eq!(node.dirs.len(), 1);
        assert!(node.dirs.contains_key("src"));
        let src = &node.dirs["src"];
        assert_eq!(src.files.len(), 1);
        assert_eq!(src.files[0].0, "main.rs");
    }

    #[test]
    fn dir_node_insert_deeply_nested() {
        let mut node = DirNode::default();
        node.insert(".claude/commands/dev.md", "content");
        assert!(node.files.is_empty());
        assert!(node.dirs.contains_key(".claude"));
        let claude = &node.dirs[".claude"];
        assert!(claude.dirs.contains_key("commands"));
        let commands = &claude.dirs["commands"];
        assert_eq!(commands.files.len(), 1);
        assert_eq!(commands.files[0].0, "dev.md");
    }

    #[test]
    fn dir_node_insert_multiple_files_same_dir() {
        let mut node = DirNode::default();
        node.insert("src/main.rs", "fn main() {}");
        node.insert("src/lib.rs", "pub mod app;");
        let src = &node.dirs["src"];
        assert_eq!(src.files.len(), 2);
    }

    #[test]
    fn dir_node_insert_mixed_files_and_dirs() {
        let mut node = DirNode::default();
        node.insert("README.md", "readme");
        node.insert("src/main.rs", "main");
        node.insert("src/config/mod.rs", "config");
        assert_eq!(node.files.len(), 1);
        assert_eq!(node.dirs.len(), 1);
        let src = &node.dirs["src"];
        assert_eq!(src.files.len(), 1);
        assert_eq!(src.dirs.len(), 1);
    }

    #[tokio::test]
    async fn init_bare_repo_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let result = init_bare_repo(tmp.path(), "testuser", "testrepo", "main").await;
        assert!(result.is_ok(), "init_bare_repo should succeed: {result:?}");
        let repo_path = result.unwrap();
        assert!(repo_path.exists());
        assert!(repo_path.join("HEAD").exists());
        // Check HEAD points to main
        let head_content = tokio::fs::read_to_string(repo_path.join("HEAD"))
            .await
            .unwrap();
        assert!(head_content.contains("refs/heads/main"));
    }

    #[tokio::test]
    async fn init_bare_repo_with_custom_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let result = init_bare_repo(tmp.path(), "testuser", "customrepo", "develop").await;
        assert!(result.is_ok());
        let repo_path = result.unwrap();
        let head_content = tokio::fs::read_to_string(repo_path.join("HEAD"))
            .await
            .unwrap();
        assert!(head_content.contains("refs/heads/develop"));
    }

    #[tokio::test]
    async fn init_bare_repo_with_files_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let files = vec![templates::TemplateFile {
            path: "hello.txt",
            content: "Hello, World!".into(),
        }];
        let result = init_bare_repo_with_files(tmp.path(), "alice", "myapp", "main", &files).await;
        assert!(
            result.is_ok(),
            "init_bare_repo_with_files should succeed: {result:?}"
        );
        let repo_path = result.unwrap();
        assert!(repo_path.exists());
    }

    #[tokio::test]
    async fn hash_object_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        // First init a bare repo so hash-object has a valid git dir
        let _ = tokio::process::Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(tmp.path())
            .output()
            .await
            .unwrap();

        let hash = hash_object(tmp.path(), "test content").await;
        assert!(hash.is_ok(), "hash_object should succeed: {hash:?}");
        let hash = hash.unwrap();
        assert_eq!(hash.len(), 40, "git hash should be 40 hex chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn mktree_with_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = tokio::process::Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(tmp.path())
            .output()
            .await
            .unwrap();

        // First create a blob, then use it in mktree
        let blob_hash = hash_object(tmp.path(), "content").await.unwrap();
        let entry = format!("100644 blob {blob_hash}\ttest.txt");
        let result = mktree(tmp.path(), &[entry]).await;
        assert!(
            result.is_ok(),
            "mktree with one entry should succeed: {result:?}"
        );
        let tree_hash = result.unwrap();
        assert_eq!(tree_hash.len(), 40);
        assert!(tree_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
