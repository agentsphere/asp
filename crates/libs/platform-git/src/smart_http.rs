// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Smart HTTP git transport handlers.
//!
//! Provides `router()` returning an axum `Router` generic over
//! [`GitServerServices`](crate::server_services::GitServerServices).

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::response::Response;
use axum::routing::{get, post};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::error::GitError;
use crate::hooks;
use crate::pkt_line;
use crate::protection;
use crate::server_services::{GitServerServices, GitServerState};
use crate::types::{GitUser, PushEvent, ResolvedProject, TagEvent};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InfoRefsQuery {
    service: Option<String>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the smart HTTP git router.
///
/// Routes: `/{owner}/{repo}/info/refs`, `/{owner}/{repo}/git-upload-pack`,
/// `/{owner}/{repo}/git-receive-pack`.
pub fn router<Svc: GitServerServices>() -> Router<GitServerState<Svc>> {
    Router::new()
        .route("/{owner}/{repo}/info/refs", get(info_refs::<Svc>))
        .route("/{owner}/{repo}/git-upload-pack", post(upload_pack::<Svc>))
        .route(
            "/{owner}/{repo}/git-receive-pack",
            post(receive_pack::<Svc>),
        )
        .layer(axum::middleware::map_response(add_www_authenticate))
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/// Add `WWW-Authenticate: Basic` to 401 responses on git routes.
///
/// Placed only on the git smart HTTP router so the browser SPA doesn't
/// get a native credentials dialog for API 401s.
pub async fn add_www_authenticate(response: Response) -> Response {
    if response.status() == axum::http::StatusCode::UNAUTHORIZED {
        let (mut parts, body) = response.into_parts();
        parts.headers.insert(
            axum::http::header::WWW_AUTHENTICATE,
            "Basic realm=\"platform\""
                .parse()
                .expect("valid header value"),
        );
        Response::from_parts(parts, body)
    } else {
        response
    }
}

/// Extract username and password from HTTP Basic Auth header.
pub fn extract_basic_credentials(headers: &HeaderMap) -> Result<(String, String), GitError> {
    let auth_value = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(GitError::Unauthorized)?;

    let encoded = auth_value
        .strip_prefix("Basic ")
        .ok_or(GitError::Unauthorized)?;

    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
        .map_err(|_| GitError::Unauthorized)?;

    let decoded_str = String::from_utf8(decoded).map_err(|_| GitError::Unauthorized)?;

    let (username, password_raw) = decoded_str.split_once(':').ok_or(GitError::Unauthorized)?;

    if username.is_empty() {
        return Err(GitError::Unauthorized);
    }

    Ok((username.to_owned(), password_raw.to_owned()))
}

/// Run a git service (upload-pack or receive-pack) with bidirectional streaming.
///
/// Spawns a git subprocess, pipes the request body to stdin, and streams
/// stdout as the response body.
pub fn run_git_service(
    repo_path: &Path,
    service: &str,
    body: Body,
    timeout_secs: u64,
) -> Result<Response, GitError> {
    let mut child = tokio::process::Command::new("git")
        .arg(service)
        .arg("--stateless-rpc")
        .arg(repo_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(anyhow::anyhow!("failed to spawn git: {e}")))?;

    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");

    let git_timeout = std::time::Duration::from_secs(timeout_secs);
    tokio::spawn(async move {
        let result = tokio::time::timeout(git_timeout, async {
            let mut body_stream = body.into_data_stream();
            while let Some(frame_result) = body_stream.next().await {
                let frame = frame_result.map_err(|e| anyhow::anyhow!("body read: {e}"))?;
                stdin.write_all(&frame).await?;
            }
            stdin.shutdown().await?;
            Ok::<(), anyhow::Error>(())
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "stdin pipe failed"),
            Err(_) => tracing::warn!("git upload-pack stdin pipe timed out"),
        }
    });

    let stream = ReaderStream::new(stdout);
    let response_body = Body::from_stream(stream);

    let content_type = format!("application/x-git-{service}-result");
    Ok(Response::builder()
        .header("Content-Type", content_type)
        .header("Cache-Control", "no-cache")
        .body(response_body)
        .expect("response builder"))
}

// ---------------------------------------------------------------------------
// Access checks
// ---------------------------------------------------------------------------

/// Check access for an HTTP git operation. Returns the authenticated user (if any).
///
/// For read operations on public repos, returns `Ok(None)` (no auth needed).
async fn check_access<Svc: GitServerServices>(
    svc: &Arc<Svc>,
    headers: &HeaderMap,
    project: &ResolvedProject,
    is_read: bool,
) -> Result<Option<GitUser>, GitError> {
    // Public repos: allow unauthenticated reads
    if is_read && project.visibility == "public" {
        return Ok(None);
    }

    let (username, password) = extract_basic_credentials(headers)?;
    let git_user = svc.authenticate_basic(&username, &password).await?;
    svc.check_git_rate(&git_user.user_name).await?;

    check_access_for_user(svc, &git_user, project, is_read).await?;
    Ok(Some(git_user))
}

/// Check RBAC access for an already-authenticated git user.
///
/// Enforces token scope (project + workspace), visibility rules, and permission checks.
/// Returns `Ok(())` if allowed, `Err(NotFound)` if denied (to avoid leaking repo existence).
pub async fn check_access_for_user<Svc: GitServerServices>(
    svc: &Arc<Svc>,
    git_user: &GitUser,
    project: &ResolvedProject,
    is_read: bool,
) -> Result<(), GitError> {
    // Enforce hard project scope from API token
    if let Some(scope_pid) = git_user.boundary_project_id
        && scope_pid != project.project_id
    {
        return Err(GitError::NotFound("repository".into()));
    }

    // Enforce hard workspace scope from API token
    if let Some(scope_wid) = git_user.boundary_workspace_id {
        let in_workspace = svc
            .check_workspace_boundary(project.project_id, scope_wid)
            .await?;
        if !in_workspace {
            return Err(GitError::NotFound("repository".into()));
        }
    }

    // Public or internal repos: any authenticated user can read
    if is_read && (project.visibility == "public" || project.visibility == "internal") {
        return Ok(());
    }

    if is_read {
        svc.check_read(git_user, project).await?;
    } else {
        svc.check_write(git_user, project).await?;
    }

    Ok(())
}

/// Check branch protection rules for all ref updates in a push.
pub async fn enforce_push_protection<Svc: GitServerServices>(
    svc: &Arc<Svc>,
    project: &ResolvedProject,
    git_user: &GitUser,
    ref_updates: &[hooks::RefUpdate],
) -> Result<(), GitError> {
    for update in ref_updates {
        let Some(branch) = update.refname.strip_prefix("refs/heads/") else {
            continue;
        };
        let rule = svc
            .get_protection(project.project_id, branch)
            .await
            .map_err(|e| GitError::Other(anyhow::anyhow!("protection check: {e}")))?;

        let Some(rule) = rule else { continue };

        let is_admin = svc
            .check_admin_or_owner(git_user.user_id, project, rule.allow_admin_bypass)
            .await
            .unwrap_or(false);

        if is_admin {
            continue;
        }

        if rule.require_pr {
            return Err(GitError::Forbidden);
        }

        if rule.block_force_push
            && protection::is_force_push(&project.repo_disk_path, &update.old_sha, &update.new_sha)
                .await
        {
            return Err(GitError::Forbidden);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /:owner/:repo/info/refs?service=git-upload-pack|git-receive-pack`
#[tracing::instrument(skip(state, headers), fields(%owner, %repo))]
async fn info_refs<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    Query(query): Query<InfoRefsQuery>,
    headers: HeaderMap,
) -> Result<Response, GitError> {
    let service = query
        .service
        .as_deref()
        .ok_or_else(|| GitError::BadRequest("service query parameter required".into()))?;

    if service != "git-upload-pack" && service != "git-receive-pack" {
        return Err(GitError::BadRequest("invalid service".into()));
    }

    let project = state.svc.resolve(&owner, &repo).await?;

    if let Err(e) = check_access(&state.svc, &headers, &project, service == "git-upload-pack").await
    {
        match &e {
            GitError::Unauthorized => {
                tracing::debug!(%owner, %repo, "git info/refs: no credentials");
            }
            _ => {
                tracing::error!(error = %e, %owner, %repo, "git info/refs auth failed");
            }
        }
        return Err(e);
    }

    let git_cmd = service.strip_prefix("git-").unwrap_or(service);
    let output = tokio::process::Command::new("git")
        .arg(git_cmd)
        .arg("--stateless-rpc")
        .arg("--advertise-refs")
        .arg(&project.repo_disk_path)
        .output()
        .await
        .map_err(|e| GitError::Other(anyhow::anyhow!("failed to spawn git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(stderr = %stderr, "git info/refs failed");
        return Err(GitError::Other(anyhow::anyhow!("git failed: {stderr}")));
    }

    let mut body = pkt_line::pkt_line_header(service);
    body.extend_from_slice(&output.stdout);

    let content_type = format!("application/x-{service}-advertisement");
    Ok(Response::builder()
        .header("Content-Type", content_type)
        .header("Cache-Control", "no-cache")
        .body(Body::from(body))
        .expect("response builder"))
}

/// `POST /:owner/:repo/git-upload-pack`
#[tracing::instrument(skip(state, request), fields(%owner, %repo), err)]
async fn upload_pack<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    request: Request,
) -> Result<Response, GitError> {
    let project = state.svc.resolve(&owner, &repo).await?;
    check_access(&state.svc, request.headers(), &project, true).await?;

    run_git_service(
        &project.repo_disk_path,
        "upload-pack",
        request.into_body(),
        state.config.git_http_timeout_secs,
    )
}

/// `POST /:owner/:repo/git-receive-pack`
#[tracing::instrument(skip(state, request), fields(%owner, %repo), err)]
#[allow(clippy::too_many_lines)]
async fn receive_pack<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    request: Request,
) -> Result<Response, GitError> {
    let project = state.svc.resolve(&owner, &repo).await?;

    let git_user = check_access(&state.svc, request.headers(), &project, false)
        .await?
        .expect("receive-pack always authenticates");

    let body = request.into_body();

    // Buffer pkt-line header (ref commands) until flush-pkt, then enforce
    // branch protection before streaming PACK data to git.
    let mut pkt_buf = Vec::new();
    let mut body_stream = body.into_data_stream();
    let mut remaining_frame: Option<bytes::Bytes> = None;

    loop {
        let frame = match body_stream.next().await {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                return Err(GitError::Other(anyhow::anyhow!("body read: {e}")));
            }
            None => {
                return Err(GitError::BadRequest("incomplete pack data".into()));
            }
        };
        pkt_buf.extend_from_slice(&frame);

        if let Some(flush_pos) = pkt_line::find_flush_pkt(&pkt_buf) {
            if flush_pos < pkt_buf.len() {
                remaining_frame = Some(bytes::Bytes::copy_from_slice(&pkt_buf[flush_pos..]));
                pkt_buf.truncate(flush_pos);
            }
            break;
        }

        if pkt_buf.len() > 1_048_576 {
            return Err(GitError::BadRequest("pack header too large".into()));
        }
    }

    let ref_updates = hooks::parse_pack_commands(&pkt_buf);
    let pushed_branches = hooks::extract_pushed_branches(&ref_updates);

    enforce_push_protection(&state.svc, &project, &git_user, &ref_updates).await?;

    // Spawn git receive-pack
    let mut child = tokio::process::Command::new("git")
        .arg("receive-pack")
        .arg("--stateless-rpc")
        .arg(&project.repo_disk_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(anyhow::anyhow!("failed to spawn git: {e}")))?;

    let mut stdin = child.stdin.take().expect("stdin piped");
    let mut stdout = child.stdout.take().expect("stdout piped");

    let git_timeout = std::time::Duration::from_secs(state.config.git_http_timeout_secs);

    let git_result = tokio::time::timeout(git_timeout, async {
        let (stdin_result, stdout_bytes) = tokio::join!(
            async {
                stdin.write_all(&pkt_buf).await?;
                if let Some(remaining) = remaining_frame {
                    stdin.write_all(&remaining).await?;
                }
                while let Some(frame_result) = body_stream.next().await {
                    let frame = frame_result.map_err(std::io::Error::other)?;
                    stdin.write_all(&frame).await?;
                }
                stdin.shutdown().await?;
                Ok::<(), std::io::Error>(())
            },
            async {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await?;
                Ok::<Vec<u8>, std::io::Error>(buf)
            }
        );
        stdin_result.map_err(|e| anyhow::anyhow!("stdin write: {e}"))?;
        let output = stdout_bytes.map_err(|e| anyhow::anyhow!("stdout read: {e}"))?;
        let status = child
            .wait()
            .await
            .map_err(|e| anyhow::anyhow!("git wait: {e}"))?;
        Ok::<(Vec<u8>, std::process::ExitStatus), anyhow::Error>((output, status))
    })
    .await;

    let (output, status) = match git_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(GitError::Other(e)),
        Err(_elapsed) => {
            let _ = child.kill().await;
            return Err(GitError::Other(anyhow::anyhow!(
                "git receive-pack timed out after {}s",
                state.config.git_http_timeout_secs
            )));
        }
    };

    if status.success() {
        tracing::info!(
            %owner, %repo,
            branches = ?pushed_branches,
            "receive-pack succeeded, dispatching post-receive"
        );
        let svc = state.svc.clone();
        let pushed_tags = hooks::extract_pushed_tags(&ref_updates);
        let user_id = git_user.user_id;
        let user_name = git_user.user_name.clone();
        let project_clone = project.clone();
        tokio::spawn(async move {
            dispatch_post_receive(
                &svc,
                user_id,
                &user_name,
                &project_clone,
                pushed_branches,
                pushed_tags,
            )
            .await;
        });
    }

    state.svc.audit_git_push(
        git_user.user_id,
        &git_user.user_name,
        project.project_id,
        git_user.ip_addr.as_deref(),
    );

    Ok(Response::builder()
        .header("Content-Type", "application/x-git-receive-pack-result")
        .header("Cache-Control", "no-cache")
        .body(Body::from(output))
        .expect("response builder"))
}

/// Dispatch post-receive hooks (push + tag events).
async fn dispatch_post_receive<Svc: GitServerServices>(
    svc: &Arc<Svc>,
    user_id: uuid::Uuid,
    user_name: &str,
    project: &ResolvedProject,
    pushed_branches: Vec<String>,
    pushed_tags: Vec<String>,
) {
    for branch in &pushed_branches {
        let event = PushEvent {
            project_id: project.project_id,
            user_id,
            user_name: user_name.to_string(),
            repo_path: project.repo_disk_path.clone(),
            branch: branch.clone(),
            commit_sha: None,
        };
        if let Err(e) = svc.on_push(&event).await {
            tracing::error!(error = %e, branch = %branch, "post-receive push hook failed");
        }
    }
    for tag in &pushed_tags {
        let event = TagEvent {
            project_id: project.project_id,
            user_id,
            user_name: user_name.to_string(),
            repo_path: project.repo_disk_path.clone(),
            tag_name: tag.clone(),
            commit_sha: None,
        };
        if let Err(e) = svc.on_tag(&event).await {
            tracing::error!(error = %e, tag = %tag, "post-receive tag hook failed");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_basic_credentials --

    #[test]
    fn extract_basic_credentials_valid() {
        let mut headers = HeaderMap::new();
        // base64("alice:secret123") = "YWxpY2U6c2VjcmV0MTIz"
        headers.insert(AUTHORIZATION, "Basic YWxpY2U6c2VjcmV0MTIz".parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "alice");
        assert_eq!(pass, "secret123");
    }

    #[test]
    fn extract_basic_credentials_missing_header() {
        let headers = HeaderMap::new();
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_not_basic() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer token123".parse().unwrap());
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_password_with_colon() {
        let mut headers = HeaderMap::new();
        // base64("alice:pass:word") = "YWxpY2U6cGFzczp3b3Jk"
        headers.insert(AUTHORIZATION, "Basic YWxpY2U6cGFzczp3b3Jk".parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "alice");
        assert_eq!(pass, "pass:word");
    }

    #[test]
    fn extract_basic_credentials_empty_username_rejected() {
        let mut headers = HeaderMap::new();
        // base64(":password") = "OnBhc3N3b3Jk"
        headers.insert(AUTHORIZATION, "Basic OnBhc3N3b3Jk".parse().unwrap());
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_empty_password_accepted() {
        let mut headers = HeaderMap::new();
        // base64("alice:") = "YWxpY2U6"
        headers.insert(AUTHORIZATION, "Basic YWxpY2U6".parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "alice");
        assert_eq!(pass, "");
    }

    #[test]
    fn extract_basic_credentials_no_colon_rejected() {
        let mut headers = HeaderMap::new();
        // base64("justausername") = "anVzdGF1c2VybmFtZQ=="
        headers.insert(AUTHORIZATION, "Basic anVzdGF1c2VybmFtZQ==".parse().unwrap());
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_invalid_base64() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Basic !!!invalid!!!".parse().unwrap());
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_invalid_utf8() {
        let mut headers = HeaderMap::new();
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &[0xff, 0xfe]);
        headers.insert(AUTHORIZATION, format!("Basic {encoded}").parse().unwrap());
        assert!(extract_basic_credentials(&headers).is_err());
    }

    #[test]
    fn extract_basic_credentials_long_password() {
        let mut headers = HeaderMap::new();
        let long_pass = "x".repeat(1000);
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("user:{long_pass}"),
        );
        headers.insert(AUTHORIZATION, format!("Basic {encoded}").parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "user");
        assert_eq!(pass.len(), 1000);
    }

    #[test]
    fn extract_basic_credentials_special_chars_in_username() {
        let mut headers = HeaderMap::new();
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            "user@org:password",
        );
        headers.insert(AUTHORIZATION, format!("Basic {encoded}").parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "user@org");
        assert_eq!(pass, "password");
    }

    #[test]
    fn extract_basic_credentials_multiple_colons_in_password() {
        let mut headers = HeaderMap::new();
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "user:a:b:c:d");
        headers.insert(AUTHORIZATION, format!("Basic {encoded}").parse().unwrap());
        let (user, pass) = extract_basic_credentials(&headers).unwrap();
        assert_eq!(user, "user");
        assert_eq!(pass, "a:b:c:d");
    }

    // -- add_www_authenticate --

    #[tokio::test]
    async fn add_www_authenticate_adds_header_on_401() {
        let response = Response::builder()
            .status(axum::http::StatusCode::UNAUTHORIZED)
            .body(Body::empty())
            .unwrap();
        let result = add_www_authenticate(response).await;
        assert_eq!(result.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(
            result
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .unwrap()
                .to_str()
                .unwrap(),
            "Basic realm=\"platform\""
        );
    }

    #[tokio::test]
    async fn add_www_authenticate_passes_through_non_401() {
        let response = Response::builder()
            .status(axum::http::StatusCode::OK)
            .body(Body::empty())
            .unwrap();
        let result = add_www_authenticate(response).await;
        assert_eq!(result.status(), axum::http::StatusCode::OK);
        assert!(
            result
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .is_none()
        );
    }

    #[tokio::test]
    async fn add_www_authenticate_passes_through_403() {
        let response = Response::builder()
            .status(axum::http::StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
        let result = add_www_authenticate(response).await;
        assert_eq!(result.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(
            result
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .is_none()
        );
    }

    #[tokio::test]
    async fn add_www_authenticate_passes_through_500() {
        let response = Response::builder()
            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap();
        let result = add_www_authenticate(response).await;
        assert!(
            result
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .is_none()
        );
    }

    #[tokio::test]
    async fn add_www_authenticate_passes_through_404() {
        let response = Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
        let result = add_www_authenticate(response).await;
        assert!(
            result
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .is_none()
        );
    }

    // -- InfoRefsQuery --

    #[test]
    fn info_refs_query_with_service() {
        let q: InfoRefsQuery =
            serde_json::from_value(serde_json::json!({"service": "git-upload-pack"})).unwrap();
        assert_eq!(q.service.as_deref(), Some("git-upload-pack"));
    }

    #[test]
    fn info_refs_query_without_service() {
        let q: InfoRefsQuery = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(q.service.is_none());
    }

    #[test]
    fn info_refs_query_debug() {
        let q = InfoRefsQuery {
            service: Some("git-upload-pack".into()),
        };
        let debug = format!("{q:?}");
        assert!(debug.contains("InfoRefsQuery"));
    }
}
