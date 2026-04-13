// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Repository browser API handlers.
//!
//! Provides axum routes for browsing project and ops repos (tree, blob,
//! branches, commits) with optional GPG signature verification.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::GitError;
use crate::browser_types::{BranchInfo, CommitInfo, TreeEntry};
use crate::server_services::{BrowserUser, GitServerServices, GitServerState, GpgKeyInfo};
use crate::signature::{self, SignatureInfo, SignatureStatus};
use crate::validation;

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Response for blob content.
#[derive(Debug, Serialize)]
pub struct BlobResponse {
    pub path: String,
    pub size: i64,
    pub content: String,
    pub encoding: String, // "utf-8" or "base64"
}

/// Query parameters for tree listing.
#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    #[serde(rename = "ref", default = "default_ref")]
    pub git_ref: String,
    #[serde(default)]
    pub path: String,
}

/// Query parameters for blob retrieval.
#[derive(Debug, Deserialize)]
pub struct BlobQuery {
    #[serde(rename = "ref", default = "default_ref")]
    pub git_ref: String,
    pub path: String,
}

/// Query parameters for commit listing.
#[derive(Debug, Deserialize)]
pub struct CommitsQuery {
    #[serde(rename = "ref", default = "default_ref")]
    pub git_ref: String,
    pub limit: Option<i64>,
    #[serde(default)]
    pub verify_signatures: bool,
}

fn default_ref() -> String {
    "HEAD".to_owned()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the browser API router.
pub fn router<Svc: GitServerServices>() -> Router<GitServerState<Svc>> {
    Router::new()
        .route("/api/projects/{id}/tree", get(tree::<Svc>))
        .route("/api/projects/{id}/blob", get(blob::<Svc>))
        .route("/api/projects/{id}/branches", get(branches::<Svc>))
        .route("/api/projects/{id}/commits", get(commits::<Svc>))
        .route(
            "/api/projects/{id}/commits/{sha}",
            get(commit_detail::<Svc>),
        )
        .route("/api/projects/{id}/ops-repo/tree", get(ops_tree::<Svc>))
        .route("/api/projects/{id}/ops-repo/blob", get(ops_blob::<Svc>))
        .route(
            "/api/projects/{id}/ops-repo/branches",
            get(ops_branches::<Svc>),
        )
}

// ---------------------------------------------------------------------------
// Input validation (returns GitError)
// ---------------------------------------------------------------------------

fn validate_git_ref(git_ref: &str) -> Result<(), GitError> {
    validation::validate_git_ref(git_ref)
}

fn validate_path(path: &str) -> Result<(), GitError> {
    validation::validate_path(path)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

async fn check_project_read<Svc: GitServerServices>(
    state: &GitServerState<Svc>,
    auth: &BrowserUser,
    project_id: Uuid,
) -> Result<(), GitError> {
    // Enforce hard project scope from API token
    if let Some(boundary_pid) = auth.boundary_project_id
        && boundary_pid != project_id
    {
        return Err(GitError::NotFound("project".into()));
    }

    // Enforce hard workspace scope
    if let Some(scope_wid) = auth.boundary_workspace_id {
        let in_workspace = state
            .svc
            .check_workspace_boundary(project_id, scope_wid)
            .await?;
        if !in_workspace {
            return Err(GitError::NotFound("project".into()));
        }
    }

    state
        .svc
        .check_project_read_scoped(auth.user_id, project_id, auth.token_scopes.as_deref())
        .await
}

// ---------------------------------------------------------------------------
// Git subprocess helpers
// ---------------------------------------------------------------------------

/// Run `git ls-tree -l` and parse the output.
pub async fn git_ls_tree(
    repo_path: &std::path::Path,
    git_ref: &str,
    path: &str,
) -> Result<Vec<TreeEntry>, GitError> {
    let treeish = if path.is_empty() || path == "/" {
        git_ref.to_owned()
    } else {
        let clean_path = path.trim_start_matches('/');
        format!("{git_ref}:{clean_path}")
    };

    let output = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("ls-tree")
            .arg("-l")
            .arg("--")
            .arg(&treeish)
            .output()
    })
    .await
    .map_err(|_| GitError::Other(anyhow::anyhow!("git ls-tree timed out after 30s")))?
    .map_err(|e| GitError::Other(anyhow::anyhow!("failed to run git ls-tree: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Not a valid object name") || stderr.contains("not a tree object") {
            return Err(GitError::NotFound("tree path".into()));
        }
        return Err(GitError::Other(anyhow::anyhow!(
            "git ls-tree failed: {stderr}"
        )));
    }

    Ok(parse_ls_tree(&String::from_utf8_lossy(&output.stdout)))
}

/// Run `git show ref:path` and return the blob content.
pub async fn git_show_blob(
    repo_path: &std::path::Path,
    git_ref: &str,
    path: &str,
) -> Result<BlobResponse, GitError> {
    const MAX_BLOB_SIZE: usize = 50 * 1024 * 1024;
    let clean_path = path.trim_start_matches('/');
    let object_spec = format!("{git_ref}:{clean_path}");

    let output = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("show")
            .arg(&object_spec)
            .output()
    })
    .await
    .map_err(|_| GitError::Other(anyhow::anyhow!("git show timed out after 30s")))?
    .map_err(|e| GitError::Other(anyhow::anyhow!("failed to run git show: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist") || stderr.contains("not a valid object") {
            return Err(GitError::NotFound("blob".into()));
        }
        return Err(GitError::Other(anyhow::anyhow!(
            "git show failed: {stderr}"
        )));
    }

    if output.stdout.len() > MAX_BLOB_SIZE {
        return Err(GitError::BadRequest(format!(
            "file too large: {} bytes (max {MAX_BLOB_SIZE})",
            output.stdout.len()
        )));
    }

    #[allow(clippy::cast_possible_wrap)]
    let size = output.stdout.len() as i64;

    let (content, encoding) = match String::from_utf8(output.stdout.clone()) {
        Ok(text) => (text, "utf-8".to_owned()),
        Err(_) => (
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &output.stdout),
            "base64".to_owned(),
        ),
    };

    Ok(BlobResponse {
        path: path.to_owned(),
        size,
        content,
        encoding,
    })
}

/// Run `git for-each-ref` and return branches.
pub async fn git_list_branches(repo_path: &std::path::Path) -> Result<Vec<BranchInfo>, GitError> {
    let output = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("for-each-ref")
            .arg("--format=%(refname:short)\t%(objectname:short)\t%(creatordate:iso-strict)")
            .arg("refs/heads/")
            .output()
    })
    .await
    .map_err(|_| GitError::Other(anyhow::anyhow!("git for-each-ref timed out after 30s")))?
    .map_err(|e| GitError::Other(anyhow::anyhow!("failed to run git for-each-ref: {e}")))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    Ok(parse_branches(&String::from_utf8_lossy(&output.stdout)))
}

// ---------------------------------------------------------------------------
// Handlers — project repos
// ---------------------------------------------------------------------------

/// `GET /api/projects/:id/tree?ref=main&path=/`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn tree<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<TreeEntry>>, GitError> {
    check_project_read(&state, &auth, id).await?;
    validate_git_ref(&query.git_ref)?;
    validate_path(&query.path)?;

    let (repo_path, _) = state.svc.get_project_repo_path(id).await?;
    Ok(Json(
        git_ls_tree(&repo_path, &query.git_ref, &query.path).await?,
    ))
}

/// `GET /api/projects/:id/blob?ref=main&path=src/main.rs`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn blob<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
    Query(query): Query<BlobQuery>,
) -> Result<Json<BlobResponse>, GitError> {
    check_project_read(&state, &auth, id).await?;
    validate_git_ref(&query.git_ref)?;
    validate_path(&query.path)?;
    if query.path.is_empty() {
        return Err(GitError::BadRequest("path is required".into()));
    }
    let (repo_path, _) = state.svc.get_project_repo_path(id).await?;
    Ok(Json(
        git_show_blob(&repo_path, &query.git_ref, &query.path).await?,
    ))
}

/// `GET /api/projects/:id/branches`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn branches<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BranchInfo>>, GitError> {
    check_project_read(&state, &auth, id).await?;
    let (repo_path, _) = state.svc.get_project_repo_path(id).await?;
    Ok(Json(git_list_branches(&repo_path).await?))
}

/// `GET /api/projects/:id/commits?ref=main&limit=20`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn commits<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
    Query(query): Query<CommitsQuery>,
) -> Result<Json<Vec<CommitInfo>>, GitError> {
    check_project_read(&state, &auth, id).await?;
    validate_git_ref(&query.git_ref)?;

    let (repo_path, _) = state.svc.get_project_repo_path(id).await?;

    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let output = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("log")
            .arg(format!("-n{limit}"))
            .arg("--format=%H%x00%s%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI")
            .arg(&query.git_ref)
            .arg("--")
            .output()
    })
    .await
    .map_err(|_| GitError::Other(anyhow::anyhow!("git log timed out after 30s")))?
    .map_err(|e| GitError::Other(anyhow::anyhow!("failed to run git log: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown revision")
            || stderr.contains("bad default revision")
            || stderr.contains("bad revision")
        {
            return Ok(Json(Vec::new()));
        }
        return Err(GitError::Other(anyhow::anyhow!("git log failed: {stderr}")));
    }

    let mut commits_list = parse_log(&String::from_utf8_lossy(&output.stdout));

    if query.verify_signatures {
        let shas: Vec<String> = commits_list.iter().map(|c| c.sha.clone()).collect();
        let sigs = verify_commits_batch(&state, &repo_path, id, &shas).await;
        for (commit, sig) in commits_list.iter_mut().zip(sigs) {
            commit.signature = Some(sig);
        }
    }

    Ok(Json(commits_list))
}

/// `GET /api/projects/:id/commits/:sha`
#[tracing::instrument(skip(state), fields(%id, %sha), err)]
async fn commit_detail<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path((id, sha)): Path<(Uuid, String)>,
) -> Result<Json<CommitInfo>, GitError> {
    check_project_read(&state, &auth, id).await?;

    if !signature::validate_commit_sha(&sha) {
        return Err(GitError::BadRequest("invalid commit SHA".into()));
    }

    let (repo_path, _) = state.svc.get_project_repo_path(id).await?;

    let output = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("log")
            .arg("-n1")
            .arg("--format=%H%x00%s%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI")
            .arg(&sha)
            .arg("--")
            .output()
    })
    .await
    .map_err(|_| GitError::Other(anyhow::anyhow!("git log timed out after 30s")))?
    .map_err(|e| GitError::Other(anyhow::anyhow!("failed to run git log: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown revision")
            || stderr.contains("bad default revision")
            || stderr.contains("bad object")
        {
            return Err(GitError::NotFound("commit".into()));
        }
        return Err(GitError::Other(anyhow::anyhow!("git log failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits_list = parse_log(&stdout);
    if commits_list.is_empty() {
        return Err(GitError::NotFound("commit".into()));
    }

    let mut commit = commits_list.remove(0);
    commit.signature = Some(verify_single_commit(&state, &repo_path, id, &commit.sha).await);

    Ok(Json(commit))
}

// ---------------------------------------------------------------------------
// Handlers — ops repos
// ---------------------------------------------------------------------------

/// `GET /api/projects/:id/ops-repo/tree?ref=main&path=/`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn ops_tree<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<TreeEntry>>, GitError> {
    check_project_read(&state, &auth, id).await?;
    validate_git_ref(&query.git_ref)?;
    validate_path(&query.path)?;
    let (repo_path, _) = state.svc.get_ops_repo_path(id).await?;
    Ok(Json(
        git_ls_tree(&repo_path, &query.git_ref, &query.path).await?,
    ))
}

/// `GET /api/projects/:id/ops-repo/blob?ref=main&path=...`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn ops_blob<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
    Query(query): Query<BlobQuery>,
) -> Result<Json<BlobResponse>, GitError> {
    check_project_read(&state, &auth, id).await?;
    validate_git_ref(&query.git_ref)?;
    validate_path(&query.path)?;
    if query.path.is_empty() {
        return Err(GitError::BadRequest("path is required".into()));
    }
    let (repo_path, _) = state.svc.get_ops_repo_path(id).await?;
    Ok(Json(
        git_show_blob(&repo_path, &query.git_ref, &query.path).await?,
    ))
}

/// `GET /api/projects/:id/ops-repo/branches`
#[tracing::instrument(skip(state), fields(%id), err)]
async fn ops_branches<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    auth: BrowserUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BranchInfo>>, GitError> {
    check_project_read(&state, &auth, id).await?;
    let (repo_path, _) = state.svc.get_ops_repo_path(id).await?;
    Ok(Json(git_list_branches(&repo_path).await?))
}

// ---------------------------------------------------------------------------
// Signature verification
// ---------------------------------------------------------------------------

/// Verify a single commit's signature.
async fn verify_single_commit<Svc: GitServerServices>(
    state: &GitServerState<Svc>,
    repo_path: &std::path::Path,
    project_id: Uuid,
    sha: &str,
) -> SignatureInfo {
    // Check cache first
    let cache_key = format!("gpg:sig:{project_id}:{sha}");
    if let Ok(Some(cached_json)) = state.svc.sig_cache_get(&cache_key).await
        && let Ok(info) = serde_json::from_str::<SignatureInfo>(&cached_json)
    {
        return info;
    }

    let info = do_verify_commit(state, repo_path, sha).await;

    // Cache the result (5 min TTL)
    if let Ok(json) = serde_json::to_string(&info) {
        let _ = state.svc.sig_cache_set(&cache_key, &json, 300).await;
    }

    info
}

/// Perform the actual signature verification against the git repo.
async fn do_verify_commit<Svc: GitServerServices>(
    state: &GitServerState<Svc>,
    repo_path: &std::path::Path,
    sha: &str,
) -> SignatureInfo {
    let Some(raw_commit) = git_cat_file_commit(repo_path, sha).await else {
        return no_signature();
    };

    let Some(parsed) = signature::parse_commit_gpgsig(&raw_commit) else {
        return no_signature();
    };

    let Some(key_id) = signature::extract_signing_key_id(&parsed.signature_armor) else {
        return bad_signature(None, None);
    };

    let row = match state.svc.lookup_gpg_key(&key_id).await {
        Ok(Some(info)) => info,
        _ => return bad_signature(Some(key_id), None),
    };

    verify_against_key(&parsed, &raw_commit, &key_id, row).await
}

/// Run `git cat-file commit <sha>` and return the raw output.
async fn git_cat_file_commit(repo_path: &std::path::Path, sha: &str) -> Option<Vec<u8>> {
    let result = tokio::time::timeout(GIT_TIMEOUT, {
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("cat-file")
            .arg("commit")
            .arg(sha)
            .output()
    })
    .await;

    match result {
        Ok(Ok(out)) if out.status.success() => Some(out.stdout),
        _ => None,
    }
}

/// Verify the commit signature against a stored GPG key.
async fn verify_against_key(
    parsed: &signature::ParsedCommitSignature,
    raw_commit: &[u8],
    key_id: &str,
    row: GpgKeyInfo,
) -> SignatureInfo {
    use pgp::composed::{Deserializable, SignedPublicKey};

    let Ok(public_key) = SignedPublicKey::from_bytes(std::io::Cursor::new(&row.public_key_bytes))
    else {
        return bad_signature(Some(key_id.to_owned()), Some(row.fingerprint));
    };

    let sig_armor = parsed.signature_armor.clone();
    let signed_data = parsed.signed_data.clone();
    let pk = public_key.clone();
    let valid = tokio::task::spawn_blocking(move || {
        signature::verify_signature(&sig_armor, &signed_data, &pk)
    })
    .await
    .unwrap_or(false);

    if !valid {
        return bad_signature(Some(key_id.to_owned()), Some(row.fingerprint));
    }

    let signer_name = public_key
        .details
        .users
        .first()
        .map(|u| u.id.id().to_string());

    let author_email = extract_author_email_from_commit(raw_commit);
    let email_match = author_email
        .as_ref()
        .is_some_and(|email| row.emails.iter().any(|ke| ke.eq_ignore_ascii_case(email)));

    let status = if email_match {
        SignatureStatus::Verified
    } else {
        SignatureStatus::UnverifiedSigner
    };

    SignatureInfo {
        status,
        signer_key_id: Some(key_id.to_owned()),
        signer_fingerprint: Some(row.fingerprint),
        signer_name,
    }
}

/// Verify signatures for a batch of commits in parallel.
async fn verify_commits_batch<Svc: GitServerServices>(
    state: &GitServerState<Svc>,
    repo_path: &std::path::Path,
    project_id: Uuid,
    shas: &[String],
) -> Vec<SignatureInfo> {
    let futures: Vec<_> = shas
        .iter()
        .map(|sha| verify_single_commit(state, repo_path, project_id, sha))
        .collect();
    futures_util::future::join_all(futures).await
}

fn no_signature() -> SignatureInfo {
    SignatureInfo {
        status: SignatureStatus::NoSignature,
        signer_key_id: None,
        signer_fingerprint: None,
        signer_name: None,
    }
}

fn bad_signature(key_id: Option<String>, fingerprint: Option<String>) -> SignatureInfo {
    SignatureInfo {
        status: SignatureStatus::BadSignature,
        signer_key_id: key_id,
        signer_fingerprint: fingerprint,
        signer_name: None,
    }
}

// ---------------------------------------------------------------------------
// Parsers (pure functions)
// ---------------------------------------------------------------------------

/// Extract the author email from a raw commit object.
pub fn extract_author_email_from_commit(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("author ")
            && let Some(start) = rest.find('<')
            && let Some(end) = rest[start..].find('>')
        {
            return Some(rest[start + 1..start + end].to_owned());
        }
    }
    None
}

/// Parse `git ls-tree -l` output.
///
/// Format: `<mode> <type> <sha> <size>\t<name>`
/// Size is `-` for trees.
pub fn parse_ls_tree(output: &str) -> Vec<TreeEntry> {
    output
        .lines()
        .filter_map(|line| {
            let (meta, name) = line.split_once('\t')?;
            let parts: Vec<&str> = meta.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let size = parts[3].parse::<i64>().ok();
            Some(TreeEntry {
                mode: parts[0].to_owned(),
                entry_type: parts[1].to_owned(),
                sha: parts[2].to_owned(),
                size,
                name: name.to_owned(),
            })
        })
        .collect()
}

/// Parse `git for-each-ref` output for branches.
///
/// Format: `<name>\t<sha>\t<date>`
pub fn parse_branches(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let name = parts.next()?.to_owned();
            let sha = parts.next()?.to_owned();
            let updated_at = parts.next().unwrap_or_default().to_owned();
            Some(BranchInfo {
                name,
                sha,
                updated_at,
            })
        })
        .collect()
}

/// Parse `git log` output with null-delimited fields.
///
/// Format per line: `sha\0subject\0author_name\0author_email\0author_date\0committer_name\0committer_email\0committer_date`
pub fn parse_log(output: &str) -> Vec<CommitInfo> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(8, '\0').collect();
            if parts.len() < 8 {
                return None;
            }
            Some(CommitInfo {
                sha: parts[0].to_owned(),
                message: parts[1].to_owned(),
                author_name: parts[2].to_owned(),
                author_email: parts[3].to_owned(),
                authored_at: parts[4].to_owned(),
                committer_name: parts[5].to_owned(),
                committer_email: parts[6].to_owned(),
                committed_at: parts[7].to_owned(),
                signature: None,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_ls_tree --

    #[test]
    fn parse_ls_tree_normal() {
        let output = "100644 blob abc1234 1234\tREADME.md\n040000 tree def5678      -\tsrc\n";
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
    fn parse_ls_tree_malformed_line() {
        let output = "100644 blob abc1234 1234 README.md\n";
        let entries = parse_ls_tree(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_ls_tree_too_few_meta_parts() {
        let output = "100644 blob\tREADME.md\n";
        let entries = parse_ls_tree(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_ls_tree_symlink_mode() {
        let output = "120000 blob abc1234     25\tsrc/link\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mode, "120000");
    }

    #[test]
    fn parse_ls_tree_submodule() {
        let output = "160000 commit abc1234      -\texternal/dep\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, "commit");
        assert!(entries[0].size.is_none());
    }

    #[test]
    fn parse_ls_tree_executable_file() {
        let output = "100755 blob abc1234 5678\tscripts/deploy.sh\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mode, "100755");
    }

    #[test]
    fn parse_ls_tree_filename_with_spaces() {
        let output = "100644 blob abc1234 100\tfile with spaces.txt\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file with spaces.txt");
    }

    #[test]
    fn parse_ls_tree_mixed_valid_invalid() {
        let output = "100644 blob abc1234 1234\tvalid.txt\nbad line without tab\n100644 blob def5678 5678\talso-valid.rs\n";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 2);
    }

    // -- parse_branches --

    #[test]
    fn parse_branches_normal() {
        let output = "main\tabc1234\t2026-02-19T10:00:00+00:00\nfeature\tdef5678\t2026-02-18T09:00:00+00:00\n";
        let branches = parse_branches(output);
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[1].name, "feature");
    }

    #[test]
    fn parse_branches_empty() {
        assert!(parse_branches("").is_empty());
    }

    #[test]
    fn parse_branches_single() {
        let output = "main\tabc1234\t2026-02-19T10:00:00+00:00\n";
        let branches = parse_branches(output);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].updated_at, "2026-02-19T10:00:00+00:00");
    }

    #[test]
    fn parse_branches_missing_date() {
        let output = "main\tabc1234\n";
        let branches = parse_branches(output);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].updated_at, "");
    }

    #[test]
    fn parse_branches_only_name() {
        let output = "lonely\n";
        let branches = parse_branches(output);
        assert!(branches.is_empty());
    }

    // -- parse_log --

    #[test]
    fn parse_log_normal() {
        let output = "abc123\0Initial commit\0Alice\0alice@example.com\x002026-02-19T10:00:00+00:00\0Alice\0alice@example.com\x002026-02-19T10:00:00+00:00\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha, "abc123");
        assert_eq!(commits[0].message, "Initial commit");
        assert!(commits[0].signature.is_none());
    }

    #[test]
    fn parse_log_empty() {
        assert!(parse_log("").is_empty());
    }

    #[test]
    fn parse_log_too_few_fields() {
        let output = "abc123\0msg\0alice\0alice@e.com\02026-01-01\n";
        assert!(parse_log(output).is_empty());
    }

    #[test]
    fn parse_log_multiple_commits() {
        let line1 = "aaa\0msg1\0a\0a@e.com\02026-01-01\0c\0c@e.com\02026-01-01";
        let line2 = "bbb\0msg2\0b\0b@e.com\02026-01-02\0c\0c@e.com\02026-01-02";
        let output = format!("{line1}\n{line2}\n");
        let commits = parse_log(&output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "aaa");
        assert_eq!(commits[1].sha, "bbb");
    }

    // -- extract_author_email --

    #[test]
    fn extract_author_email_standard_format() {
        let raw = b"tree abc123\nauthor Alice <alice@example.com> 1700000000 +0000\ncommitter Bob <bob@example.com> 1700000000 +0000\n\ncommit message\n";
        let email = extract_author_email_from_commit(raw);
        assert_eq!(email.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn extract_author_email_no_author_line() {
        let raw =
            b"tree abc123\ncommitter Bob <bob@example.com> 1700000000 +0000\n\ncommit message\n";
        assert!(extract_author_email_from_commit(raw).is_none());
    }

    #[test]
    fn extract_author_email_no_angle_brackets() {
        let raw = b"tree abc123\nauthor Alice 1700000000 +0000\n\ncommit message\n";
        assert!(extract_author_email_from_commit(raw).is_none());
    }

    #[test]
    fn extract_author_email_empty_input() {
        assert!(extract_author_email_from_commit(b"").is_none());
    }

    #[test]
    fn extract_author_email_complex_name() {
        let raw =
            b"tree abc123\nauthor John Q. Public Jr. <john.public@company.org> 1700000000 +0000\n";
        let email = extract_author_email_from_commit(raw);
        assert_eq!(email.as_deref(), Some("john.public@company.org"));
    }

    #[test]
    fn extract_author_email_multiple_author_lines() {
        let raw = b"tree abc\nauthor First <first@e.com> 1 +0\nauthor Second <second@e.com> 1 +0\n";
        let email = extract_author_email_from_commit(raw);
        assert_eq!(email.as_deref(), Some("first@e.com"));
    }

    // -- no_signature / bad_signature --

    #[test]
    fn no_signature_helper() {
        let info = no_signature();
        assert_eq!(info.status, SignatureStatus::NoSignature);
        assert!(info.signer_key_id.is_none());
    }

    #[test]
    fn bad_signature_helper_no_key_info() {
        let info = bad_signature(None, None);
        assert_eq!(info.status, SignatureStatus::BadSignature);
    }

    #[test]
    fn bad_signature_helper_with_key_info() {
        let info = bad_signature(Some("ABC123".into()), Some("DEADBEEF".into()));
        assert_eq!(info.signer_key_id.as_deref(), Some("ABC123"));
        assert_eq!(info.signer_fingerprint.as_deref(), Some("DEADBEEF"));
    }

    // -- query types --

    #[test]
    fn tree_query_defaults() {
        let q: TreeQuery = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(q.git_ref, "HEAD");
        assert_eq!(q.path, "");
    }

    #[test]
    fn blob_query_defaults() {
        let q: BlobQuery = serde_json::from_value(serde_json::json!({"path": "file.rs"})).unwrap();
        assert_eq!(q.git_ref, "HEAD");
    }

    #[test]
    fn commits_query_defaults() {
        let q: CommitsQuery = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(q.git_ref, "HEAD");
        assert!(q.limit.is_none());
        assert!(!q.verify_signatures);
    }

    #[test]
    fn commits_query_with_all_fields() {
        let q: CommitsQuery = serde_json::from_value(serde_json::json!({
            "ref": "develop",
            "limit": 50,
            "verify_signatures": true
        }))
        .unwrap();
        assert_eq!(q.git_ref, "develop");
        assert_eq!(q.limit, Some(50));
        assert!(q.verify_signatures);
    }

    #[test]
    fn default_ref_is_head() {
        assert_eq!(default_ref(), "HEAD");
    }

    // -- BlobResponse --

    #[test]
    fn blob_response_serialization() {
        let resp = BlobResponse {
            path: "src/main.rs".into(),
            size: 42,
            content: "fn main() {}".into(),
            encoding: "utf-8".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["encoding"], "utf-8");
        assert_eq!(json["size"], 42);
    }

    #[test]
    fn blob_response_debug() {
        let resp = BlobResponse {
            path: "main.rs".into(),
            size: 10,
            content: "hello".into(),
            encoding: "utf-8".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("BlobResponse"));
    }
}
