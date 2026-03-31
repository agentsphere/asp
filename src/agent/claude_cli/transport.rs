// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::control::ControlRequest;
use super::error::CliError;
use super::messages::{CliMessage, CliUserInput, parse_cli_message};

/// Subprocess transport for the Claude CLI NDJSON protocol.
///
/// Spawns `claude` as a child process with `--input-format stream-json
/// --output-format stream-json`. Provides methods to send/receive NDJSON
/// messages over stdin/stdout.
pub struct SubprocessTransport {
    pub(crate) child: Child,
    pub(crate) stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    pub(crate) stdout: Mutex<BufReader<ChildStdout>>,
    pub(crate) stderr_task: Option<JoinHandle<String>>,
    pub(crate) session_id: Mutex<Option<String>>,
    pub(crate) alive: std::sync::atomic::AtomicBool,
}

/// Options for spawning the Claude CLI subprocess.
///
/// All fields are optional — reasonable defaults are used when absent.
#[derive(Default)]
pub struct CliSpawnOptions {
    /// Override CLI binary path.
    pub cli_path: Option<PathBuf>,
    /// Working directory for the CLI process.
    pub cwd: Option<PathBuf>,
    /// `--model` flag.
    pub model: Option<String>,
    /// `--system-prompt` flag.
    pub system_prompt: Option<String>,
    /// `--append-system-prompt` flag.
    pub append_system_prompt: Option<String>,
    /// `--allowedTools` flag (comma-separated tool names).
    pub allowed_tools: Option<Vec<String>>,
    /// `--permission-mode` flag (e.g. "bypassPermissions").
    pub permission_mode: Option<String>,
    /// `--max-turns` flag.
    pub max_turns: Option<u32>,
    /// `--resume <session-id>` to continue a previous conversation.
    pub resume_session: Option<String>,
    /// `--mcp-config <path>` for MCP server configuration.
    pub mcp_config: Option<PathBuf>,
    /// `--include-partial-messages` for streaming partial tokens.
    pub include_partial: bool,
    /// `CLAUDE_CONFIG_DIR` env var.
    pub config_dir: Option<PathBuf>,
    /// `CLAUDE_CODE_OAUTH_TOKEN` env var (subscription auth).
    pub oauth_token: Option<String>,
    /// `ANTHROPIC_API_KEY` env var (API key auth — fallback).
    pub anthropic_api_key: Option<String>,
    /// Additional environment variables to pass to the subprocess.
    pub extra_env: Vec<(String, String)>,
    /// `-p <text>` — one-shot prompt mode. When set, `--input-format stream-json`
    /// is omitted from args (stdin is not used in `-p` mode).
    pub prompt: Option<String>,
    /// `--session-id <id>` — set CLI session ID (first invocation).
    pub initial_session_id: Option<String>,
    /// `--json-schema <json>` — force structured output.
    pub json_schema: Option<String>,
    /// `--tools ""` — disable all built-in tools.
    pub disable_tools: bool,
}

impl SubprocessTransport {
    /// Spawn the Claude CLI as a subprocess.
    ///
    /// **Security:** Uses `Command::env_clear()` then adds ONLY whitelisted vars
    /// (PATH, HOME, TMPDIR, auth vars, `CLAUDE_CONFIG_DIR`, `extra_env`).
    /// This prevents leaking `DATABASE_URL`, `PLATFORM_MASTER_KEY`, etc.
    #[allow(clippy::needless_pass_by_value)]
    pub fn spawn(opts: CliSpawnOptions) -> Result<Self, CliError> {
        let cli_path = find_claude_cli(opts.cli_path.as_deref())?;
        let args = build_args(&opts);
        let env_vars = build_env(&opts);

        let env_keys: Vec<&str> = env_vars.iter().map(|(k, _)| k.as_str()).collect();
        tracing::info!(
            cli_path = %cli_path.display(),
            args = ?args,
            env_keys = ?env_keys,
            cwd = ?opts.cwd,
            "spawning Claude CLI subprocess"
        );

        let mut cmd = tokio::process::Command::new(&cli_path);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear(); // Security: start with empty environment

        // Apply whitelisted environment variables
        for (key, value) in &env_vars {
            cmd.env(key, value);
        }

        if let Some(ref cwd) = opts.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().map_err(CliError::SpawnFailed)?;

        let stdin = child.stdin.take().ok_or(CliError::NotRunning)?;
        let stdout = child.stdout.take().ok_or(CliError::NotRunning)?;
        let stderr = child.stderr.take();

        // Spawn a task to capture stderr for error reporting
        let stderr_task = stderr.map(|stderr| {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                let mut collected = String::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.is_empty() {
                        tracing::warn!(target: "claude_cli_stderr", "{}", line);
                        if collected.len() < 4096 && !collected.is_empty() {
                            collected.push('\n');
                        }
                        if collected.len() < 4096 {
                            collected.push_str(&line);
                        }
                    }
                }
                collected
            })
        });

        Ok(Self {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        })
    }

    /// Send a user text message to the CLI via stdin.
    pub async fn send_message(&self, content: &str) -> Result<(), CliError> {
        let input = CliUserInput::text(content);
        self.write_json(&input).await
    }

    /// Send structured content (multi-part, images) via stdin.
    pub async fn send_structured(&self, content: serde_json::Value) -> Result<(), CliError> {
        let input = CliUserInput::structured(content);
        self.write_json(&input).await
    }

    /// Close stdin, signaling EOF to the child process.
    ///
    /// Call this in `-p` (one-shot prompt) mode where stdin is not needed,
    /// to prevent the CLI from blocking on stdin reads. Dropping the
    /// `ChildStdin` handle closes the pipe fd, delivering EOF.
    pub async fn close_stdin(&self) {
        let mut guard = self.stdin.lock().await;
        drop(guard.take());
    }

    /// Read the next NDJSON message from stdout.
    ///
    /// Returns `Ok(None)` when stdout closes (process exited).
    /// Skips unknown message types and empty lines.
    pub async fn recv(&self) -> Result<Option<CliMessage>, CliError> {
        let mut stdout = self.stdout.lock().await;
        loop {
            let mut line = String::new();
            let bytes_read = stdout
                .read_line(&mut line)
                .await
                .map_err(CliError::StdoutRead)?;

            if bytes_read == 0 {
                self.alive
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return Ok(None);
            }

            match parse_cli_message(&line) {
                Ok(Some(msg)) => {
                    // Track session ID from system init
                    if let CliMessage::System(ref sys) = msg {
                        let mut sid = self.session_id.lock().await;
                        *sid = Some(sys.session_id.clone());
                    }
                    return Ok(Some(msg));
                }
                Ok(None) => {
                    // Unknown type or empty line — skip
                }
                Err(e) => {
                    tracing::warn!(line = %line.trim(), error = %e, "skipping invalid NDJSON line from CLI");
                }
            }
        }
    }

    /// Send a control request (interrupt, `set_model`, etc.).
    pub async fn send_control(&self, request: ControlRequest) -> Result<(), CliError> {
        self.write_json(&request).await
    }

    /// Get the CLI session ID (available after receiving the System init message).
    pub async fn session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    /// Kill the subprocess.
    pub async fn kill(&mut self) -> Result<(), CliError> {
        self.alive
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.child
            .kill()
            .await
            .map_err(|e| CliError::SessionError(format!("failed to kill CLI process: {e}")))
    }

    /// Check if the subprocess is still running.
    pub fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Wait for the process to exit and return the exit code + stderr.
    pub async fn wait(mut self) -> Result<(i32, String), CliError> {
        let status = self
            .child
            .wait()
            .await
            .map_err(|e| CliError::SessionError(format!("wait failed: {e}")))?;

        self.alive
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let stderr = if let Some(task) = self.stderr_task.take() {
            task.await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "stderr capture task panicked");
                String::new()
            })
        } else {
            String::new()
        };

        Ok((status.code().unwrap_or(-1), stderr))
    }

    /// Write a JSON value followed by newline to stdin.
    async fn write_json(&self, value: &impl serde::Serialize) -> Result<(), CliError> {
        if !self.is_alive() {
            return Err(CliError::NotRunning);
        }
        let mut json = serde_json::to_string(value).map_err(|e| {
            CliError::StdinWrite(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        json.push('\n');
        let mut guard = self.stdin.lock().await;
        let stdin = guard.as_mut().ok_or(CliError::NotRunning)?;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(CliError::StdinWrite)?;
        stdin.flush().await.map_err(CliError::StdinWrite)?;
        Ok(())
    }
}

impl Drop for SubprocessTransport {
    fn drop(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = self.child.start_kill();
    }
}

// ---------------------------------------------------------------------------
// CLI discovery
// ---------------------------------------------------------------------------

/// Find the `claude` CLI binary.
///
/// Priority:
/// 1. Explicit path from `CliSpawnOptions`
/// 2. `CLAUDE_CLI_PATH` env var
/// 3. PATH lookup via `which`
/// 4. Common npm global install paths
/// 5. `/usr/local/bin/claude`
fn find_claude_cli(explicit: Option<&Path>) -> Result<PathBuf, CliError> {
    // 1. Explicit path
    if let Some(path) = explicit {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(CliError::CliNotFound);
    }

    // 2. CLAUDE_CLI_PATH env var
    if let Ok(path) = std::env::var("CLAUDE_CLI_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 3. PATH lookup
    if let Ok(output) = std::process::Command::new("which").arg("claude").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    // 4. Common npm global paths
    let npm_paths = ["/usr/local/bin/claude", "/usr/bin/claude"];
    for path in &npm_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(CliError::CliNotFound)
}

/// Build CLI arguments from spawn options.
pub(crate) fn build_args(opts: &CliSpawnOptions) -> Vec<String> {
    let mut args = Vec::new();

    // When using -p mode, skip --input-format stream-json (stdin not used)
    if opts.prompt.is_none() {
        args.push("--input-format".to_owned());
        args.push("stream-json".to_owned());
    }

    args.push("--output-format".to_owned());
    args.push("stream-json".to_owned());
    args.push("--verbose".to_owned());

    if let Some(ref model) = opts.model {
        args.push("--model".to_owned());
        args.push(model.clone());
    }

    if let Some(ref system_prompt) = opts.system_prompt {
        args.push("--system-prompt".to_owned());
        args.push(system_prompt.clone());
    }

    if let Some(ref append) = opts.append_system_prompt {
        args.push("--append-system-prompt".to_owned());
        args.push(append.clone());
    }

    if let Some(ref tools) = opts.allowed_tools {
        args.push("--allowedTools".to_owned());
        args.push(tools.join(","));
    }

    if let Some(ref mode) = opts.permission_mode {
        args.push("--permission-mode".to_owned());
        args.push(mode.clone());
    }

    if let Some(max_turns) = opts.max_turns {
        args.push("--max-turns".to_owned());
        args.push(max_turns.to_string());
    }

    if let Some(ref session_id) = opts.resume_session {
        args.push("--resume".to_owned());
        args.push(session_id.clone());
    }

    if let Some(ref path) = opts.mcp_config {
        args.push("--mcp-config".to_owned());
        args.push(path.display().to_string());
    }

    if opts.include_partial {
        args.push("--include-partial-messages".to_owned());
    }

    if opts.disable_tools {
        args.push("--tools".to_owned());
        args.push(String::new()); // --tools ""
    }

    if let Some(ref schema) = opts.json_schema {
        args.push("--json-schema".to_owned());
        args.push(schema.clone());
    }

    if let Some(ref prompt) = opts.prompt {
        args.push("-p".to_owned());
        args.push(prompt.clone());
    }

    if let Some(ref sid) = opts.initial_session_id {
        args.push("--session-id".to_owned());
        args.push(sid.clone());
    }

    args
}

/// Build the whitelisted environment variables for the subprocess.
///
/// **Security:** Only these env vars are passed to the CLI process.
pub(crate) fn build_env(opts: &CliSpawnOptions) -> Vec<(String, String)> {
    let mut env = Vec::new();

    // System essentials
    if let Ok(path) = std::env::var("PATH") {
        env.push(("PATH".to_owned(), path));
    }
    if let Ok(home) = std::env::var("HOME") {
        env.push(("HOME".to_owned(), home));
    }
    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        env.push(("TMPDIR".to_owned(), tmpdir));
    }

    // Non-interactive mode signal
    env.push(("TERM".to_owned(), "dumb".to_owned()));

    // Skip all interactive prompts (permission dialogs, onboarding, etc.)
    env.push(("CI".to_owned(), "true".to_owned()));

    // Identity — some tools need USER
    if let Ok(user) = std::env::var("USER") {
        env.push(("USER".to_owned(), user));
    }

    // XDG dirs — CLI may use these for config/data discovery
    if let Ok(val) = std::env::var("XDG_CONFIG_HOME") {
        env.push(("XDG_CONFIG_HOME".to_owned(), val));
    }
    if let Ok(val) = std::env::var("XDG_DATA_HOME") {
        env.push(("XDG_DATA_HOME".to_owned(), val));
    }

    // Auth: prefer OAuth token, fall back to API key
    if let Some(ref token) = opts.oauth_token {
        env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), token.clone()));
    } else if let Some(ref key) = opts.anthropic_api_key {
        env.push(("ANTHROPIC_API_KEY".to_owned(), key.clone()));
    }

    // Config dir
    if let Some(ref dir) = opts.config_dir {
        env.push(("CLAUDE_CONFIG_DIR".to_owned(), dir.display().to_string()));
    }

    // Extra env vars from caller
    for (key, value) in &opts.extra_env {
        env.push((key.clone(), value.clone()));
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_cli_explicit_path_exists() {
        // Use a path we know exists on all platforms
        let path = PathBuf::from("/usr/bin/env");
        if path.exists() {
            let result = find_claude_cli(Some(&path));
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), path);
        }
    }

    #[test]
    fn find_cli_explicit_path_missing() {
        let path = PathBuf::from("/nonexistent/path/to/claude");
        let result = find_claude_cli(Some(&path));
        assert!(matches!(result, Err(CliError::CliNotFound)));
    }

    #[test]
    fn spawn_options_default() {
        let opts = CliSpawnOptions::default();
        assert!(opts.cli_path.is_none());
        assert!(opts.cwd.is_none());
        assert!(opts.model.is_none());
        assert!(opts.system_prompt.is_none());
        assert!(opts.max_turns.is_none());
        assert!(opts.oauth_token.is_none());
        assert!(opts.anthropic_api_key.is_none());
        assert!(!opts.include_partial);
    }

    #[test]
    fn build_args_includes_stream_flags() {
        let opts = CliSpawnOptions::default();
        let args = build_args(&opts);
        assert!(args.contains(&"--input-format".to_owned()));
        assert!(args.contains(&"stream-json".to_owned()));
        assert!(args.contains(&"--output-format".to_owned()));
        assert!(args.contains(&"--verbose".to_owned()));
    }

    #[test]
    fn build_args_with_model() {
        let opts = CliSpawnOptions {
            model: Some("opus".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--model".to_owned()));
        assert!(args.contains(&"opus".to_owned()));
    }

    #[test]
    fn build_args_with_max_turns() {
        let opts = CliSpawnOptions {
            max_turns: Some(10),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--max-turns".to_owned()));
        assert!(args.contains(&"10".to_owned()));
    }

    #[test]
    fn build_args_with_resume_session() {
        let opts = CliSpawnOptions {
            resume_session: Some("session-abc".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--resume".to_owned()));
        assert!(args.contains(&"session-abc".to_owned()));
    }

    #[test]
    fn build_args_with_mcp_config() {
        let opts = CliSpawnOptions {
            mcp_config: Some(PathBuf::from("/tmp/mcp.json")),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--mcp-config".to_owned()));
        assert!(args.contains(&"/tmp/mcp.json".to_owned()));
    }

    #[test]
    fn build_args_with_permission_mode() {
        let opts = CliSpawnOptions {
            permission_mode: Some("bypassPermissions".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--permission-mode".to_owned()));
        assert!(args.contains(&"bypassPermissions".to_owned()));
    }

    #[test]
    fn build_args_with_prompt_skips_input_format() {
        let opts = CliSpawnOptions {
            prompt: Some("hello world".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        // -p mode should NOT include --input-format stream-json
        assert!(
            !args
                .windows(2)
                .any(|w| w[0] == "--input-format" && w[1] == "stream-json")
        );
        assert!(args.contains(&"-p".to_owned()));
        assert!(args.contains(&"hello world".to_owned()));
        // --output-format stream-json is still present
        assert!(args.contains(&"--output-format".to_owned()));
    }

    #[test]
    fn build_args_with_initial_session_id() {
        let opts = CliSpawnOptions {
            initial_session_id: Some("my-session-123".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--session-id".to_owned()));
        assert!(args.contains(&"my-session-123".to_owned()));
    }

    #[test]
    fn build_args_with_json_schema() {
        let opts = CliSpawnOptions {
            json_schema: Some(r#"{"type":"object"}"#.into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--json-schema".to_owned()));
        assert!(args.contains(&r#"{"type":"object"}"#.to_owned()));
    }

    #[test]
    fn build_args_with_disable_tools() {
        let opts = CliSpawnOptions {
            disable_tools: true,
            ..Default::default()
        };
        let args = build_args(&opts);
        let idx = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[idx + 1], ""); // --tools ""
    }

    #[test]
    fn build_args_include_partial() {
        let opts = CliSpawnOptions {
            include_partial: true,
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--include-partial-messages".to_owned()));
    }

    #[test]
    fn build_env_with_oauth_token() {
        let opts = CliSpawnOptions {
            oauth_token: Some("my-oauth-token".into()),
            ..Default::default()
        };
        let env = build_env(&opts);
        assert!(
            env.iter()
                .any(|(k, v)| k == "CLAUDE_CODE_OAUTH_TOKEN" && v == "my-oauth-token")
        );
        // API key should NOT be present when oauth token is set
        assert!(env.iter().all(|(k, _)| k != "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn build_env_api_key_fallback() {
        let opts = CliSpawnOptions {
            anthropic_api_key: Some("sk-ant-key".into()),
            ..Default::default()
        };
        let env = build_env(&opts);
        assert!(
            env.iter()
                .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v == "sk-ant-key")
        );
        // OAuth token should NOT be present when only API key is set
        assert!(env.iter().all(|(k, _)| k != "CLAUDE_CODE_OAUTH_TOKEN"));
    }

    #[test]
    fn build_env_oauth_takes_precedence() {
        let opts = CliSpawnOptions {
            oauth_token: Some("oauth-tok".into()),
            anthropic_api_key: Some("api-key".into()),
            ..Default::default()
        };
        let env = build_env(&opts);
        assert!(
            env.iter()
                .any(|(k, v)| k == "CLAUDE_CODE_OAUTH_TOKEN" && v == "oauth-tok")
        );
        assert!(env.iter().all(|(k, _)| k != "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn build_env_whitelist_has_path_home() {
        let opts = CliSpawnOptions::default();
        let env = build_env(&opts);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        // PATH and HOME should be present if set in the real environment
        // (can't guarantee in all test envs, but the function should include them)
        assert!(
            keys.contains(&"PATH") || std::env::var("PATH").is_err(),
            "PATH should be whitelisted"
        );
    }

    #[test]
    fn build_env_config_dir() {
        let opts = CliSpawnOptions {
            config_dir: Some(PathBuf::from("/tmp/claude-config")),
            ..Default::default()
        };
        let env = build_env(&opts);
        assert!(
            env.iter()
                .any(|(k, v)| k == "CLAUDE_CONFIG_DIR" && v == "/tmp/claude-config")
        );
    }

    #[test]
    fn build_env_extra_env() {
        let opts = CliSpawnOptions {
            extra_env: vec![
                ("CUSTOM_VAR".into(), "custom_value".into()),
                ("ANOTHER".into(), "val".into()),
            ],
            ..Default::default()
        };
        let env = build_env(&opts);
        assert!(
            env.iter()
                .any(|(k, v)| k == "CUSTOM_VAR" && v == "custom_value")
        );
        assert!(env.iter().any(|(k, v)| k == "ANOTHER" && v == "val"));
    }

    #[test]
    fn build_env_no_database_url() {
        // The env_clear + whitelist approach means DATABASE_URL is never included
        let opts = CliSpawnOptions::default();
        let env = build_env(&opts);
        assert!(
            env.iter().all(|(k, _)| k != "DATABASE_URL"),
            "DATABASE_URL must never be passed to CLI subprocess"
        );
        assert!(
            env.iter().all(|(k, _)| k != "PLATFORM_MASTER_KEY"),
            "PLATFORM_MASTER_KEY must never be passed to CLI subprocess"
        );
    }

    #[test]
    fn build_env_includes_ci_true() {
        let opts = CliSpawnOptions::default();
        let env = build_env(&opts);
        assert!(
            env.iter().any(|(k, v)| k == "CI" && v == "true"),
            "CI=true should always be set to skip interactive prompts"
        );
    }

    #[test]
    fn build_env_includes_term_dumb() {
        let opts = CliSpawnOptions::default();
        let env = build_env(&opts);
        assert!(
            env.iter().any(|(k, v)| k == "TERM" && v == "dumb"),
            "TERM=dumb should always be set for non-interactive mode"
        );
    }

    #[test]
    fn build_env_includes_user_when_set() {
        // USER is typically set in test environments
        if std::env::var("USER").is_ok() {
            let opts = CliSpawnOptions::default();
            let env = build_env(&opts);
            assert!(
                env.iter().any(|(k, _)| k == "USER"),
                "USER should be forwarded when set"
            );
        }
    }

    /// Helper: spawn `sh -c 'exec cat'` as a mock transport.
    /// Uses shell so that CLI args are ignored — `cat` reads pure stdin.
    fn spawn_cat_transport() -> SubprocessTransport {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "exec cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn sh -c cat");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        SubprocessTransport {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        }
    }

    #[tokio::test]
    async fn close_stdin_sends_eof() {
        let transport = spawn_cat_transport();
        transport.close_stdin().await;
        // cat should exit after stdin EOF → recv returns None
        let result = transport.recv().await.unwrap();
        assert!(result.is_none(), "cat should exit after stdin close");
    }

    #[tokio::test]
    async fn spawn_and_kill() {
        let mut transport = spawn_cat_transport();
        assert!(transport.is_alive());
        transport.kill().await.unwrap();
        assert!(!transport.is_alive());
    }

    #[tokio::test]
    async fn send_and_recv_with_cat() {
        let transport = spawn_cat_transport();

        // Write a valid NDJSON system message — cat echoes it back
        let msg = r#"{"type":"system","subtype":"init","session_id":"test-123"}"#;
        {
            let mut guard = transport.stdin.lock().await;
            let stdin = guard.as_mut().unwrap();
            stdin
                .write_all(format!("{msg}\n").as_bytes())
                .await
                .unwrap();
            stdin.flush().await.unwrap();
        }

        let received = transport.recv().await.unwrap();
        assert!(received.is_some());
        match received.unwrap() {
            CliMessage::System(s) => {
                assert_eq!(s.session_id, "test-123");
            }
            other => panic!("expected System, got: {other:?}"),
        }

        // Verify session_id was captured
        assert_eq!(transport.session_id().await.as_deref(), Some("test-123"));
    }

    #[tokio::test]
    async fn send_message_writes_ndjson() {
        let transport = spawn_cat_transport();

        // send_message writes CliUserInput JSON — cat echoes it back
        transport.send_message("hello world").await.unwrap();

        // Read raw line from stdout to verify the format
        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        stdout.read_line(&mut line).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");
        assert_eq!(parsed["message"]["content"], "hello world");
    }

    #[tokio::test]
    async fn recv_returns_none_on_eof() {
        // Spawn a process that exits immediately after printing one line
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "echo done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        // "done" is not valid JSON — will be skipped, then EOF → None
        let result = transport.recv().await.unwrap();
        assert!(result.is_none());
        assert!(!transport.is_alive());
    }

    #[tokio::test]
    async fn recv_skips_invalid_json() {
        let transport = spawn_cat_transport();

        // Write invalid JSON then valid JSON
        {
            let mut guard = transport.stdin.lock().await;
            let stdin = guard.as_mut().unwrap();
            stdin.write_all(b"not json\n").await.unwrap();
            stdin
                .write_all(br#"{"type":"system","subtype":"init","session_id":"after-invalid"}"#)
                .await
                .unwrap();
            stdin.write_all(b"\n").await.unwrap();
            stdin.flush().await.unwrap();
        }

        // Should skip the invalid line and return the valid one
        let msg = transport.recv().await.unwrap().unwrap();
        match msg {
            CliMessage::System(s) => assert_eq!(s.session_id, "after-invalid"),
            other => panic!("expected System, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn write_json_to_not_running_fails() {
        let mut transport = spawn_cat_transport();
        transport.kill().await.unwrap();

        let result = transport.send_message("hello").await;
        assert!(matches!(result, Err(CliError::NotRunning)));
    }

    #[tokio::test]
    async fn drop_kills_child_process() {
        let pid = {
            let transport = spawn_cat_transport();
            let pid = transport.child.id().expect("child should have a pid");
            assert!(transport.is_alive());
            pid
            // transport is dropped here — Drop should kill the child
        };
        // Give the OS a moment to clean up
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Verify the process is gone using `kill -0 <pid>` (checks existence without unsafe)
        let output = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .expect("kill -0 command failed");
        assert!(
            !output.status.success(),
            "child process should be killed after drop"
        );
    }

    // -- build_args: additional option combinations --

    #[test]
    fn build_args_with_system_prompt() {
        let opts = CliSpawnOptions {
            system_prompt: Some("You are a helpful assistant.".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--system-prompt".to_owned()));
        assert!(args.contains(&"You are a helpful assistant.".to_owned()));
    }

    #[test]
    fn build_args_with_append_system_prompt() {
        let opts = CliSpawnOptions {
            append_system_prompt: Some("Additional context.".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--append-system-prompt".to_owned()));
        assert!(args.contains(&"Additional context.".to_owned()));
    }

    #[test]
    fn build_args_with_allowed_tools() {
        let opts = CliSpawnOptions {
            allowed_tools: Some(vec!["Read".into(), "Write".into(), "Bash".into()]),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--allowedTools".to_owned()));
        assert!(args.contains(&"Read,Write,Bash".to_owned()));
    }

    #[test]
    fn build_args_with_all_options() {
        let opts = CliSpawnOptions {
            model: Some("opus".into()),
            system_prompt: Some("sys".into()),
            append_system_prompt: Some("append".into()),
            allowed_tools: Some(vec!["Read".into()]),
            permission_mode: Some("bypassPermissions".into()),
            max_turns: Some(5),
            resume_session: Some("sess-123".into()),
            mcp_config: Some(PathBuf::from("/tmp/mcp.json")),
            include_partial: true,
            disable_tools: true,
            json_schema: Some(r#"{"type":"object"}"#.into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--model".to_owned()));
        assert!(args.contains(&"--system-prompt".to_owned()));
        assert!(args.contains(&"--append-system-prompt".to_owned()));
        assert!(args.contains(&"--allowedTools".to_owned()));
        assert!(args.contains(&"--permission-mode".to_owned()));
        assert!(args.contains(&"--max-turns".to_owned()));
        assert!(args.contains(&"--resume".to_owned()));
        assert!(args.contains(&"--mcp-config".to_owned()));
        assert!(args.contains(&"--include-partial-messages".to_owned()));
        assert!(args.contains(&"--tools".to_owned()));
        assert!(args.contains(&"--json-schema".to_owned()));
    }

    #[test]
    fn build_args_no_resume_no_initial_session() {
        let opts = CliSpawnOptions::default();
        let args = build_args(&opts);
        assert!(!args.contains(&"--resume".to_owned()));
        assert!(!args.contains(&"--session-id".to_owned()));
    }

    #[test]
    fn build_args_prompt_with_session_id() {
        let opts = CliSpawnOptions {
            prompt: Some("test".into()),
            initial_session_id: Some("sid-abc".into()),
            ..Default::default()
        };
        let args = build_args(&opts);
        assert!(args.contains(&"-p".to_owned()));
        assert!(args.contains(&"--session-id".to_owned()));
        // --input-format should NOT be present in prompt mode
        assert!(!args.iter().any(|a| a == "--input-format"));
    }

    // -- send_structured tests --

    #[tokio::test]
    async fn send_structured_writes_json() {
        let transport = spawn_cat_transport();

        let content = serde_json::json!([
            {"type": "text", "text": "analyze this"},
            {"type": "image", "data": "base64..."}
        ]);
        transport.send_structured(content.clone()).await.unwrap();

        // Read back the echoed line
        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        stdout.read_line(&mut line).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");
        assert_eq!(parsed["message"]["content"], content);
    }

    // -- send_control tests --

    #[tokio::test]
    async fn send_control_writes_interrupt() {
        let transport = spawn_cat_transport();

        let ctrl = ControlRequest::interrupt();
        transport.send_control(ctrl).await.unwrap();

        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        stdout.read_line(&mut line).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "control");
        assert_eq!(parsed["control"]["type"], "interrupt");
    }

    #[tokio::test]
    async fn send_control_set_model() {
        let transport = spawn_cat_transport();

        let ctrl = ControlRequest::set_model("opus-4");
        transport.send_control(ctrl).await.unwrap();

        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        stdout.read_line(&mut line).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["control"]["type"], "set_model");
        assert_eq!(parsed["control"]["model"], "opus-4");
    }

    // -- wait tests --

    #[tokio::test]
    async fn wait_returns_exit_code() {
        // Spawn a process that exits with code 0
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (code, stderr) = transport.wait().await.unwrap();
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
    }

    #[tokio::test]
    async fn wait_returns_nonzero_exit_code() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "exit 42"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (code, _) = transport.wait().await.unwrap();
        assert_eq!(code, 42);
    }

    #[tokio::test]
    async fn wait_captures_stderr() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "echo 'error msg' >&2; exit 1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stderr_task = Some(tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut collected = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    collected.push_str(&line);
                }
            }
            collected
        }));

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (code, stderr_output) = transport.wait().await.unwrap();
        assert_eq!(code, 1);
        assert!(
            stderr_output.contains("error msg"),
            "stderr should contain 'error msg', got: {stderr_output}"
        );
    }

    // -- write_json error when stdin closed --

    #[tokio::test]
    async fn write_json_fails_when_stdin_dropped() {
        let transport = spawn_cat_transport();
        // Close stdin first
        transport.close_stdin().await;
        // Now try to send — stdin is None, should fail with NotRunning
        let result = transport.send_message("hello").await;
        assert!(matches!(result, Err(CliError::NotRunning)));
    }

    // -- session_id tracking --

    #[tokio::test]
    async fn session_id_initially_none() {
        let transport = spawn_cat_transport();
        assert!(transport.session_id().await.is_none());
    }

    // -- recv skips empty lines --

    #[tokio::test]
    async fn recv_skips_empty_lines() {
        let transport = spawn_cat_transport();

        {
            let mut guard = transport.stdin.lock().await;
            let stdin = guard.as_mut().unwrap();
            // Write empty lines then a valid message
            stdin.write_all(b"\n\n\n").await.unwrap();
            stdin
                .write_all(br#"{"type":"system","subtype":"init","session_id":"after-empty"}"#)
                .await
                .unwrap();
            stdin.write_all(b"\n").await.unwrap();
            stdin.flush().await.unwrap();
        }

        let msg = transport.recv().await.unwrap().unwrap();
        match msg {
            CliMessage::System(s) => assert_eq!(s.session_id, "after-empty"),
            other => panic!("expected System, got: {other:?}"),
        }
    }
}
