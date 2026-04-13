// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! SSH git server implementation.
//!
//! Provides an SSH server that handles git-upload-pack and git-receive-pack
//! commands, with branch protection enforcement for pushes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::server::{Auth, Msg, Session};
use russh::{Channel, ChannelId};
use ssh_key::{PrivateKey, PublicKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::hooks;
use crate::pkt_line;
use crate::server_config::GitServerConfig;
use crate::server_services::GitServerServices;
use crate::smart_http::{check_access_for_user, enforce_push_protection};
use crate::ssh_command::parse_ssh_command;
use crate::types::{GitUser, PushEvent, ResolvedProject, TagEvent};

// ---------------------------------------------------------------------------
// SSH push interception state machine
// ---------------------------------------------------------------------------

/// State machine for intercepting SSH push data to enforce branch protection.
pub enum SshPushState {
    /// Buffering pkt-line ref commands; not yet forwarded to git.
    Buffering(Vec<u8>),
    /// Protection check passed; forwarding data directly to git stdin.
    Forwarding,
    /// Protection check failed or error; dropping all further data.
    Rejected,
}

/// Context for an in-progress SSH push (receive-pack) operation.
struct SshPushContext {
    state: SshPushState,
    project: ResolvedProject,
    git_user: GitUser,
    ref_updates: Vec<hooks::RefUpdate>,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Spawn a git subprocess for SSH (stateful, no `--stateless-rpc`).
pub fn spawn_git(service: &str, repo_path: &Path) -> Result<tokio::process::Child, std::io::Error> {
    tokio::process::Command::new("git")
        .arg(service)
        .arg(repo_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
}

/// Pipe git subprocess stdout to SSH channel.
async fn pipe_git_to_ssh(
    stdout: &mut tokio::process::ChildStdout,
    handle: &russh::server::Handle,
    channel_id: ChannelId,
) {
    let mut buf = vec![0u8; 32768];
    loop {
        match stdout.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let data = russh::CryptoVec::from_slice(&buf[..n]);
                if handle.data(channel_id, data).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "git stdout read ended");
                break;
            }
        }
    }
}

/// Send exit status, EOF, and close on a channel (fire-and-forget).
fn send_exit_and_close(handle: russh::server::Handle, channel_id: ChannelId, code: u32) {
    tokio::spawn(async move {
        let _ = handle.exit_status_request(channel_id, code).await;
        let _ = handle.eof(channel_id).await;
        let _ = handle.close(channel_id).await;
    });
}

/// Run post-push hooks and audit logging.
async fn handle_post_push<Svc: GitServerServices>(
    svc: &Arc<Svc>,
    user_id: uuid::Uuid,
    user_name: &str,
    project: &ResolvedProject,
    ref_updates: &[hooks::RefUpdate],
) {
    let pushed_branches = hooks::extract_pushed_branches(ref_updates);
    let pushed_tags = hooks::extract_pushed_tags(ref_updates);

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
            tracing::error!(error = %e, "SSH post-receive push hook failed");
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
            tracing::error!(error = %e, "SSH post-receive tag hook failed");
        }
    }

    svc.audit_git_push(
        user_id,
        user_name,
        project.project_id,
        None, // SSH doesn't have reliable IP info
    );
}

// ---------------------------------------------------------------------------
// Host key management
// ---------------------------------------------------------------------------

/// Load an existing ED25519 host key or generate a new one via `ssh-keygen`.
pub async fn load_or_generate_host_key(path: &Path) -> Result<PrivateKey, anyhow::Error> {
    if !path.exists() {
        generate_host_key(path).await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = tokio::fs::metadata(path).await?;
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            tracing::warn!(
                path = %path.display(),
                mode = format!("{mode:04o}"),
                "SSH host key has loose permissions (should be 0600)"
            );
        }
    }

    let key = russh_keys::load_secret_key(path, None)?;
    tracing::info!(path = %path.display(), "SSH host key loaded");
    Ok(key)
}

/// Generate an ED25519 host key using `ssh-keygen`.
async fn generate_host_key(key_path: &Path) -> Result<(), anyhow::Error> {
    tracing::info!(path = %key_path.display(), "generating SSH host key");

    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let output = tokio::process::Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f"])
        .arg(key_path)
        .args(["-N", "", "-q"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("ssh-keygen failed: {stderr}"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600)).await?;
    }

    tracing::info!(path = %key_path.display(), "SSH host key generated");
    Ok(())
}

// ---------------------------------------------------------------------------
// SSH session handler
// ---------------------------------------------------------------------------

struct SshSessionHandler<Svc: GitServerServices> {
    svc: Arc<Svc>,
    git_user: Option<GitUser>,
    git_stdin: HashMap<ChannelId, tokio::process::ChildStdin>,
    push_contexts: Arc<tokio::sync::Mutex<HashMap<ChannelId, SshPushContext>>>,
}

#[async_trait::async_trait]
impl<Svc: GitServerServices> russh::server::Handler for SshSessionHandler<Svc> {
    type Error = anyhow::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fingerprint = public_key.fingerprint(ssh_key::HashAlg::Sha256).to_string();

        match self.svc.authenticate_ssh_key(&fingerprint).await {
            Ok(git_user) => {
                self.svc.update_ssh_key_last_used(&fingerprint);
                self.git_user = Some(git_user);
                Ok(Auth::Accept)
            }
            Err(_) => Ok(Auth::Reject {
                proceed_with_methods: None,
            }),
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    #[allow(clippy::too_many_lines)]
    async fn exec_request(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command_str = String::from_utf8_lossy(data);
        let parsed = match parse_ssh_command(&command_str) {
            Ok(p) => p,
            Err(e) => {
                let truncated: &str = if command_str.len() > 256 {
                    &command_str[..256]
                } else {
                    &command_str
                };
                tracing::warn!(error = %e, command = %truncated, "SSH command rejected");
                send_exit_and_close(session.handle(), channel_id, 1);
                return Ok(());
            }
        };

        let Some(git_user) = &self.git_user else {
            send_exit_and_close(session.handle(), channel_id, 1);
            return Ok(());
        };

        let Ok(project) = self.svc.resolve(&parsed.owner, &parsed.repo).await else {
            send_exit_and_close(session.handle(), channel_id, 1);
            return Ok(());
        };

        if check_access_for_user(&self.svc, git_user, &project, parsed.is_read)
            .await
            .is_err()
        {
            send_exit_and_close(session.handle(), channel_id, 1);
            return Ok(());
        }

        let service = if parsed.is_read {
            "upload-pack"
        } else {
            "receive-pack"
        };

        let mut child = match spawn_git(service, &project.repo_disk_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "failed to spawn git for SSH");
                send_exit_and_close(session.handle(), channel_id, 1);
                return Ok(());
            }
        };

        if let Some(stdin) = child.stdin.take() {
            self.git_stdin.insert(channel_id, stdin);
        }

        if !parsed.is_read {
            self.push_contexts.lock().await.insert(
                channel_id,
                SshPushContext {
                    state: SshPushState::Buffering(Vec::new()),
                    project: project.clone(),
                    git_user: git_user.clone(),
                    ref_updates: Vec::new(),
                },
            );
        }

        let mut stdout = child.stdout.take().expect("stdout piped");
        let handle = session.handle();
        let svc = self.svc.clone();
        let user_id = git_user.user_id;
        let user_name = git_user.user_name.clone();
        let is_push = !parsed.is_read;
        let push_contexts = Arc::clone(&self.push_contexts);

        tokio::spawn(Box::pin(async move {
            pipe_git_to_ssh(&mut stdout, &handle, channel_id).await;

            let exit_code = match child.wait().await {
                Ok(status) => status.code().map_or(1, i32::unsigned_abs),
                Err(_) => 1,
            };

            if is_push && exit_code == 0 {
                let ref_updates = push_contexts
                    .lock()
                    .await
                    .get(&channel_id)
                    .map(|ctx| ctx.ref_updates.clone())
                    .unwrap_or_default();
                handle_post_push(&svc, user_id, &user_name, &project, &ref_updates).await;
            }

            let _ = handle.exit_status_request(channel_id, exit_code).await;
            let _ = handle.eof(channel_id).await;
            let _ = handle.close(channel_id).await;
        }));

        Ok(())
    }

    async fn data(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let mut push_ctx = self.push_contexts.lock().await;
        if let Some(ctx) = push_ctx.get_mut(&channel_id) {
            match &mut ctx.state {
                SshPushState::Buffering(buf) => {
                    buf.extend_from_slice(data);

                    if buf.len() > 1_048_576 {
                        tracing::warn!("SSH push buffer exceeded 1MB, rejecting");
                        ctx.state = SshPushState::Rejected;
                        self.git_stdin.remove(&channel_id);
                        send_exit_and_close(session.handle(), channel_id, 1);
                        return Ok(());
                    }

                    if pkt_line::find_flush_pkt(buf).is_some() {
                        let ref_updates = hooks::parse_pack_commands(buf);
                        ctx.ref_updates = ref_updates;

                        let result = enforce_push_protection(
                            &self.svc,
                            &ctx.project,
                            &ctx.git_user,
                            &ctx.ref_updates,
                        )
                        .await;

                        match result {
                            Ok(()) => {
                                let buffered = std::mem::take(buf);
                                ctx.state = SshPushState::Forwarding;
                                if let Some(stdin) = self.git_stdin.get_mut(&channel_id) {
                                    let _ = stdin.write_all(&buffered).await;
                                }
                            }
                            Err(_e) => {
                                tracing::warn!("SSH push rejected by branch protection");
                                ctx.state = SshPushState::Rejected;
                                self.git_stdin.remove(&channel_id);
                                let msg = b"ERROR: push rejected by branch protection rules\n";
                                let _ = session
                                    .handle()
                                    .extended_data(channel_id, 1, russh::CryptoVec::from_slice(msg))
                                    .await;
                                send_exit_and_close(session.handle(), channel_id, 1);
                                return Ok(());
                            }
                        }
                    }
                }
                SshPushState::Forwarding => {
                    if let Some(stdin) = self.git_stdin.get_mut(&channel_id)
                        && stdin.write_all(data).await.is_err()
                    {
                        self.git_stdin.remove(&channel_id);
                    }
                }
                SshPushState::Rejected => {} // Drop silently
            }
            return Ok(());
        }
        drop(push_ctx);

        // Non-push: forward directly
        if let Some(stdin) = self.git_stdin.get_mut(&channel_id)
            && stdin.write_all(data).await.is_err()
        {
            self.git_stdin.remove(&channel_id);
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel_id: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.git_stdin.remove(&channel_id);
        self.push_contexts.lock().await.remove(&channel_id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Server main loop
// ---------------------------------------------------------------------------

/// Run the SSH git server. Spawned as a background task from the main binary.
///
/// Returns immediately if `config.ssh_listen_addr` is `None`.
#[tracing::instrument(skip(svc, config, cancel), err)]
pub async fn run<Svc: GitServerServices>(
    svc: Arc<Svc>,
    config: Arc<GitServerConfig>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), anyhow::Error> {
    let listen_addr = match &config.ssh_listen_addr {
        Some(addr) => addr.clone(),
        None => return Ok(()),
    };

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!(addr = %listen_addr, "SSH server listening");

    run_with_listener(svc, config, listener, &cancel).await
}

/// Run the SSH server on a pre-bound listener. Returns when shutdown is signalled.
///
/// This is the core accept loop, factored out so tests can bind to port 0
/// and discover the actual port before connecting.
pub async fn run_with_listener<Svc: GitServerServices>(
    svc: Arc<Svc>,
    config: Arc<GitServerConfig>,
    listener: tokio::net::TcpListener,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), anyhow::Error> {
    let key_path = config
        .ssh_host_key_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ssh_host_key_path required for SSH server"))?;
    let key_pair = load_or_generate_host_key(key_path).await?;

    let ssh_config = Arc::new(russh::server::Config {
        keys: vec![key_pair],
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::from_secs(0)),
        maximum_packet_size: 65536,
        ..Default::default()
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        let handler = SshSessionHandler {
                            svc: svc.clone(),
                            git_user: None,
                            git_stdin: HashMap::new(),
                            push_contexts: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                        };
                        let cfg = ssh_config.clone();
                        tokio::spawn(async move {
                            if let Err(e) = russh::server::run_stream(cfg, stream, handler).await {
                                tracing::debug!(peer = %peer_addr, error = %e, "SSH session ended");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "SSH accept failed");
                    }
                }
            }
            () = cancel.cancelled() => {
                tracing::info!("SSH server shutting down");
                break;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- SshPushState smoke tests --

    #[test]
    fn ssh_push_state_initial_is_buffering() {
        let state = SshPushState::Buffering(Vec::new());
        assert!(matches!(state, SshPushState::Buffering(_)));
    }

    #[test]
    fn ssh_push_state_forwarding() {
        let state = SshPushState::Forwarding;
        assert!(matches!(state, SshPushState::Forwarding));
    }

    #[test]
    fn ssh_push_state_rejected() {
        let state = SshPushState::Rejected;
        assert!(matches!(state, SshPushState::Rejected));
    }

    #[test]
    fn ssh_push_state_buffering_with_data() {
        let state = SshPushState::Buffering(vec![1, 2, 3, 4]);
        match state {
            SshPushState::Buffering(buf) => assert_eq!(buf.len(), 4),
            _ => panic!("expected Buffering"),
        }
    }

    // -- spawn_git --

    #[tokio::test]
    async fn spawn_git_with_nonexistent_path() {
        let result = spawn_git("upload-pack", Path::new("/tmp/nonexistent-repo.git"));
        match result {
            Ok(mut child) => {
                let _ = child.kill();
            }
            Err(_) => {
                // git not available — acceptable in some CI environments
            }
        }
    }
}
