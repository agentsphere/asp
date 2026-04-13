// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! `CliGitRepo` — default implementation of [`GitRepo`] via `git` CLI.

use std::path::Path;
use std::time::Duration;

use crate::browser_types::{BlobContent, BranchInfo, CommitInfo, TreeEntry};
use crate::error::GitError;
use crate::signature::{self, SignatureInfo, SignatureStatus};
use crate::traits::GitRepo;

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default [`GitRepo`] implementation that shells out to the `git` CLI.
pub struct CliGitRepo;

impl GitRepo for CliGitRepo {
    async fn rev_parse(&self, repo: &Path, refspec: &str) -> Result<String, GitError> {
        let output = run_git(repo, &["rev-parse", "--verify", refspec]).await?;
        Ok(output.trim().to_string())
    }

    async fn read_file(
        &self,
        repo: &Path,
        git_ref: &str,
        path: &str,
    ) -> Result<Option<String>, GitError> {
        let spec = format!("{git_ref}:{path}");
        let result = run_git(repo, &["show", &spec]).await;
        match result {
            Ok(content) => Ok(Some(content)),
            Err(GitError::CommandFailed { stderr, .. })
                if stderr.contains("does not exist")
                    || stderr.contains("not a valid object")
                    || stderr.contains("bad revision") =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    async fn list_dir(
        &self,
        repo: &Path,
        git_ref: &str,
        dir: &str,
    ) -> Result<Vec<String>, GitError> {
        let spec = if dir.is_empty() {
            format!("{git_ref}:")
        } else {
            format!("{git_ref}:{dir}")
        };
        let output = run_git(repo, &["ls-tree", "--name-only", &spec]).await?;
        Ok(output.lines().map(String::from).collect())
    }

    async fn list_tree(
        &self,
        repo: &Path,
        git_ref: &str,
        path: Option<&str>,
    ) -> Result<Vec<TreeEntry>, GitError> {
        let spec = match path {
            Some(p) if !p.is_empty() => format!("{git_ref}:{p}"),
            _ => git_ref.to_string(),
        };
        let output = run_git(repo, &["ls-tree", "-l", "--no-abbrev", &spec]).await?;
        Ok(parse_ls_tree(&output))
    }

    async fn show_blob(
        &self,
        repo: &Path,
        git_ref: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<BlobContent, GitError> {
        let spec = format!("{git_ref}:{path}");
        let output = run_git_bytes(repo, &["show", &spec]).await?;

        let is_binary = output.iter().take(8192).any(|&b| b == 0);
        let size = output.len() as i64;
        let content = if output.len() > max_bytes {
            output[..max_bytes].to_vec()
        } else {
            output
        };

        Ok(BlobContent {
            content,
            size,
            is_binary,
        })
    }

    async fn list_branches(&self, repo: &Path) -> Result<Vec<BranchInfo>, GitError> {
        let output = run_git(
            repo,
            &[
                "for-each-ref",
                "refs/heads/",
                "--format=%(refname:short)\t%(objectname)\t%(creatordate:iso8601)",
                "--sort=-creatordate",
            ],
        )
        .await?;
        Ok(parse_branches(&output))
    }

    async fn log_commits(
        &self,
        repo: &Path,
        git_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        let output = run_git(
            repo,
            &[
                "log",
                git_ref,
                &format!("--max-count={limit}"),
                &format!("--skip={offset}"),
                "--format=%H%n%s%n%an%n%ae%n%aI%n%cn%n%ce%n%cI%n---",
            ],
        )
        .await?;
        Ok(parse_log(&output))
    }

    async fn commit_detail(&self, repo: &Path, sha: &str) -> Result<CommitInfo, GitError> {
        let output = run_git(
            repo,
            &[
                "log",
                "-1",
                sha,
                "--format=%H%n%B%n---AUTHOR---%n%an%n%ae%n%aI%n---COMMITTER---%n%cn%n%ce%n%cI",
            ],
        )
        .await?;

        parse_commit_detail(&output, repo, sha).await
    }

    async fn is_ancestor(
        &self,
        repo: &Path,
        potential_ancestor: &str,
        commit: &str,
    ) -> Result<bool, GitError> {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("merge-base")
            .arg("--is-ancestor")
            .arg(potential_ancestor)
            .arg(commit)
            .output()
            .await
            .map_err(GitError::Io)?;

        Ok(output.status.success())
    }

    async fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool, GitError> {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("rev-parse")
            .arg("--verify")
            .arg(format!("refs/heads/{branch}"))
            .output()
            .await
            .map_err(GitError::Io)?;

        Ok(output.status.success())
    }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse `git ls-tree -l` output into `TreeEntry` items.
fn parse_ls_tree(output: &str) -> Vec<TreeEntry> {
    output
        .lines()
        .filter_map(|line| {
            // Format: "<mode> <type> <sha>    <size>\t<name>"
            let (meta, name) = line.split_once('\t')?;
            let parts: Vec<&str> = meta.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let size = if parts[1] == "blob" {
                parts[3].trim().parse::<i64>().ok()
            } else {
                None
            };
            Some(TreeEntry {
                mode: parts[0].to_string(),
                entry_type: parts[1].to_string(),
                sha: parts[2].to_string(),
                size,
                name: name.to_string(),
            })
        })
        .collect()
}

/// Parse `git for-each-ref` output into `BranchInfo` items.
fn parse_branches(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let name = parts.next()?.to_string();
            let sha = parts.next()?.to_string();
            let updated_at = parts.next().unwrap_or_default().to_string();
            Some(BranchInfo {
                name,
                sha,
                updated_at,
            })
        })
        .collect()
}

/// Parse `git log --format=...` output into `CommitInfo` items.
fn parse_log(output: &str) -> Vec<CommitInfo> {
    let mut commits = Vec::new();
    let entries: Vec<&str> = output.split("---\n").collect();

    for entry in entries {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let lines: Vec<&str> = entry.lines().collect();
        if lines.len() < 8 {
            continue;
        }
        commits.push(CommitInfo {
            sha: lines[0].to_string(),
            message: lines[1].to_string(),
            author_name: lines[2].to_string(),
            author_email: lines[3].to_string(),
            authored_at: lines[4].to_string(),
            committer_name: lines[5].to_string(),
            committer_email: lines[6].to_string(),
            committed_at: lines[7].to_string(),
            signature: None,
        });
    }

    commits
}

/// Parse `git log -1` detailed output and optionally verify the commit signature.
async fn parse_commit_detail(output: &str, repo: &Path, sha: &str) -> Result<CommitInfo, GitError> {
    let parts: Vec<&str> = output.splitn(2, '\n').collect();
    let commit_sha = parts
        .first()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let rest = parts.get(1).unwrap_or(&"");
    let author_parts: Vec<&str> = rest.splitn(2, "---AUTHOR---\n").collect();
    let message = author_parts
        .first()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let after_author = author_parts.get(1).unwrap_or(&"");
    let committer_parts: Vec<&str> = after_author.splitn(2, "---COMMITTER---\n").collect();

    let author_lines: Vec<&str> = committer_parts
        .first()
        .unwrap_or(&"")
        .trim()
        .lines()
        .collect();
    let committer_lines: Vec<&str> = committer_parts
        .get(1)
        .unwrap_or(&"")
        .trim()
        .lines()
        .collect();

    // Try signature verification
    let sig_info = verify_commit_signature(repo, sha).await;

    Ok(CommitInfo {
        sha: commit_sha,
        message,
        author_name: author_lines.first().unwrap_or(&"").to_string(),
        author_email: author_lines.get(1).unwrap_or(&"").to_string(),
        authored_at: author_lines.get(2).unwrap_or(&"").to_string(),
        committer_name: committer_lines.first().unwrap_or(&"").to_string(),
        committer_email: committer_lines.get(1).unwrap_or(&"").to_string(),
        committed_at: committer_lines.get(2).unwrap_or(&"").to_string(),
        signature: sig_info,
    })
}

/// Verify a commit's GPG signature by reading the raw commit object.
async fn verify_commit_signature(repo: &Path, sha: &str) -> Option<SignatureInfo> {
    let raw_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "commit", sha])
        .output()
        .await
        .ok()?;

    if !raw_output.status.success() {
        return None;
    }

    let parsed = signature::parse_commit_gpgsig(&raw_output.stdout)?;
    let key_id = signature::extract_signing_key_id(&parsed.signature_armor);

    Some(SignatureInfo {
        status: SignatureStatus::UnverifiedSigner,
        signer_key_id: key_id,
        signer_fingerprint: None,
        signer_name: None,
    })
}

// ---------------------------------------------------------------------------
// Git command runners
// ---------------------------------------------------------------------------

async fn run_git(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = tokio::time::timeout(GIT_TIMEOUT, async {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
    })
    .await
    .map_err(|_| GitError::Timeout(GIT_TIMEOUT))?
    .map_err(GitError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_git_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let output = tokio::time::timeout(GIT_TIMEOUT, async {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
    })
    .await
    .map_err(|_| GitError::Timeout(GIT_TIMEOUT))?
    .map_err(GitError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr,
        });
    }

    Ok(output.stdout)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ls_tree_basic() {
        let output = "100644 blob abc123def456789012345678901234567890abcd    1234\tREADME.md\n\
                       040000 tree def456789012345678901234567890abcd123456       -\tsrc\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "README.md");
        assert_eq!(entries[0].entry_type, "blob");
        assert_eq!(entries[0].size, Some(1234));
        assert_eq!(entries[1].name, "src");
        assert_eq!(entries[1].entry_type, "tree");
        assert_eq!(entries[1].size, None);
    }

    #[test]
    fn parse_ls_tree_empty() {
        assert!(parse_ls_tree("").is_empty());
    }

    #[test]
    fn parse_branches_basic() {
        let output = "main\tabc123\t2026-01-01 12:00:00 +0000\n\
                       develop\tdef456\t2026-01-02 12:00:00 +0000\n";
        let branches = parse_branches(output);
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[0].sha, "abc123");
        assert_eq!(branches[1].name, "develop");
    }

    #[test]
    fn parse_branches_empty() {
        assert!(parse_branches("").is_empty());
    }

    #[test]
    fn parse_log_basic() {
        let output = "abc123\nInitial commit\nAlice\nalice@example.com\n2026-01-01T12:00:00+00:00\nAlice\nalice@example.com\n2026-01-01T12:00:00+00:00\n---\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha, "abc123");
        assert_eq!(commits[0].message, "Initial commit");
        assert_eq!(commits[0].author_name, "Alice");
    }

    #[test]
    fn parse_log_empty() {
        assert!(parse_log("").is_empty());
    }

    #[test]
    fn parse_log_multiple_commits() {
        let output = "sha1\nmsg1\nA\na@e.com\n2026-01-01\nC\nc@e.com\n2026-01-01\n---\nsha2\nmsg2\nB\nb@e.com\n2026-01-02\nD\nd@e.com\n2026-01-02\n---\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "sha1");
        assert_eq!(commits[1].sha, "sha2");
    }

    #[tokio::test]
    async fn cli_git_repo_rev_parse_nonexistent() {
        let git = CliGitRepo;
        let result = git
            .rev_parse(std::path::Path::new("/nonexistent"), "HEAD")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cli_git_repo_branch_exists_nonexistent() {
        let git = CliGitRepo;
        let result = git
            .branch_exists(std::path::Path::new("/nonexistent"), "main")
            .await;
        // Returns false for nonexistent repo (command fails)
        assert!(matches!(result, Ok(false) | Err(_)));
    }
}
