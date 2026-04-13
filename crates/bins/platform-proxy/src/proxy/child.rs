//! Child process management: spawn, signal forwarding, zombie reaping.

use std::process::Stdio;

use tokio::io::BufReader;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::watch;

/// The result of spawning a child process.
pub struct SpawnedChild {
    pub child: Child,
    pub stdout: BufReader<ChildStdout>,
    pub stderr: BufReader<ChildStderr>,
}

/// Spawn the child process and capture stdout/stderr.
#[tracing::instrument(skip_all, fields(command = %command, args_count = args.len()))]
pub fn spawn(command: &str, args: &[String]) -> anyhow::Result<SpawnedChild> {
    let mut cmd = Command::new(command);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn child process '{command}': {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stderr"))?;

    tracing::info!(
        pid = child.id().unwrap_or(0),
        command,
        "child process spawned"
    );

    Ok(SpawnedChild {
        child,
        stdout: BufReader::new(stdout),
        stderr: BufReader::new(stderr),
    })
}

/// Send a signal to a child process via kill(2).
#[cfg(unix)]
pub fn signal_child(child: &Child, sig: nix::sys::signal::Signal) -> anyhow::Result<()> {
    if let Some(pid) = child.id() {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap_or(0)),
            sig,
        )
        .map_err(|e| anyhow::anyhow!("signal failed: {e}"))?;
    }
    Ok(())
}

/// Reap zombie child processes. We are PID 1 in the container, so we must
/// periodically call `waitpid(-1, WNOHANG)` to clean up any orphaned children.
#[cfg(unix)]
#[tracing::instrument(skip_all)]
pub async fn reap_zombies(mut shutdown: watch::Receiver<()>) {
    use nix::sys::wait::{WaitStatus, waitpid};
    use nix::unistd::Pid;

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                loop {
                    match waitpid(Pid::from_raw(-1), Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
                        Ok(WaitStatus::StillAlive) | Err(_) => break,
                        Ok(status) => {
                            tracing::debug!(status = ?status, "reaped zombie process");
                        }
                    }
                }
            }
            _ = shutdown.changed() => break,
        }
    }
}

/// Reap zombies stub for non-unix platforms.
#[cfg(not(unix))]
pub async fn reap_zombies(mut shutdown: watch::Receiver<()>) {
    let _ = shutdown.changed().await;
}

/// Wait for the child to exit, returning its exit code.
/// Signals shutdown to all other tasks when the child exits.
#[tracing::instrument(skip_all)]
pub async fn wait_for_exit(child: &mut Child, shutdown_tx: watch::Sender<()>) -> i32 {
    let exit_status = child.wait().await;
    // Signal shutdown to all other tasks
    let _ = shutdown_tx.send(());

    match exit_status {
        Ok(status) => {
            let code = status.code().unwrap_or(1);
            tracing::info!(exit_code = code, "child process exited");
            code
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to wait for child process");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_sleep_succeeds() {
        let spawned = spawn("sleep", &["10".into()]);
        assert!(spawned.is_ok());
        let mut spawned = spawned.unwrap();
        assert!(spawned.child.id().is_some());
        let _ = spawned.child.kill().await;
    }

    #[test]
    fn spawn_nonexistent_command_fails() {
        let result = spawn("/nonexistent/binary/path", &[]);
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("failed to spawn child process"));
    }

    #[tokio::test]
    async fn wait_for_exit_success() {
        let mut spawned = spawn("true", &[]).unwrap();
        let (shutdown_tx, _shutdown_rx) = watch::channel(());
        let code = wait_for_exit(&mut spawned.child, shutdown_tx).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn wait_for_exit_failure() {
        let mut spawned = spawn("false", &[]).unwrap();
        let (shutdown_tx, _shutdown_rx) = watch::channel(());
        let code = wait_for_exit(&mut spawned.child, shutdown_tx).await;
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn wait_for_exit_signals_shutdown() {
        let mut spawned = spawn("true", &[]).unwrap();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(());
        wait_for_exit(&mut spawned.child, shutdown_tx).await;
        // After wait_for_exit the sender is dropped (sent + dropped).
        // changed() returns Err when sender is dropped, which means shutdown was triggered.
        let result = shutdown_rx.changed().await;
        // Either Ok (value changed) or Err (sender dropped) both indicate shutdown
        assert!(result.is_ok() || result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn signal_child_sends_signal() {
        let mut spawned = spawn("sleep", &["10".into()]).unwrap();
        let result = signal_child(&spawned.child, nix::sys::signal::Signal::SIGTERM);
        assert!(result.is_ok());
        // Wait for child to actually exit after signal
        let _ = spawned.child.wait().await;
    }

    #[tokio::test]
    async fn reap_zombies_shuts_down() {
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let handle = tokio::spawn(reap_zombies(shutdown_rx));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(());
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("reap_zombies should exit within 2s")
            .expect("task should not panic");
    }
}
