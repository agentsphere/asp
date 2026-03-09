use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::error::CliError;
use crate::messages::{CliMessage, SystemMessage};
use crate::pubsub::{cli_message_to_event, dispatch_input, PubSubClient};
use crate::render;
use crate::transport::SubprocessTransport;

/// Wait for the CLI system init message within the given timeout.
pub(crate) async fn wait_for_init(
    transport: &SubprocessTransport,
    timeout_secs: u64,
) -> Result<SystemMessage, CliError> {
    let timeout = std::time::Duration::from_secs(timeout_secs);
    match tokio::time::timeout(timeout, transport.recv()).await {
        Ok(Ok(Some(CliMessage::System(sys)))) => Ok(sys),
        Ok(Ok(Some(_))) => Err(CliError::SessionError(
            "expected system init, got other message".into(),
        )),
        Ok(Ok(None)) => Err(CliError::SessionError("CLI exited before init".into())),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CliError::InitTimeout(timeout_secs)),
    }
}

/// Run the main REPL + pub/sub bridge event loop.
///
/// Merges three input sources (stdin, pub/sub, SIGTERM) and streams CLI output
/// with terminal rendering + pub/sub event publishing.
pub async fn run(
    transport: SubprocessTransport,
    pubsub: Option<PubSubClient>,
    initial_prompt: String,
    mut stdin_rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<()> {
    // 1. Register SIGTERM handler for K8s graceful shutdown
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to register SIGTERM handler")?;

    // 2. Send initial prompt, then wait for system init
    transport
        .send_message(&initial_prompt)
        .await
        .context("failed to send initial prompt to CLI")?;

    let sys = wait_for_init(&transport, 30).await?;
    render::render_message(&CliMessage::System(sys.clone()));

    // Wrap in Arc for sharing between reader task and main loop.
    // Reader task locks stdout Mutex; main loop locks stdin Mutex — no conflict.
    let transport = Arc::new(transport);

    // 3. If pub/sub: publish init event, start input subscriber
    let mut pubsub_rx = if let Some(ref ps) = pubsub {
        if let Some(event) = cli_message_to_event(&CliMessage::System(sys)) {
            if let Err(e) = ps.publish_event(&event).await {
                eprintln!("[warn] failed to publish init event: {e}");
            }
        }
        Some(ps.subscribe_input().await?)
    } else {
        None
    };

    // 4. stdin channel is now injected via parameter
    let is_tty = std::io::stdin().is_terminal();

    // 4b. Spawn CLI output reader (cancel-safe: mpsc::recv is cancel-safe,
    // unlike transport.recv() which holds a Mutex<BufReader> lock during read_line —
    // tokio's read_line is NOT cancel-safe per its docs)
    let reader_transport = transport.clone();
    let (cli_tx, mut cli_rx) =
        tokio::sync::mpsc::channel::<Result<Option<CliMessage>, CliError>>(32);
    tokio::spawn(async move {
        loop {
            let msg = reader_transport.recv().await;
            let is_eof = matches!(&msg, Ok(None));
            let is_err = msg.is_err();
            if cli_tx.send(msg).await.is_err() {
                break; // receiver dropped
            }
            if is_eof || is_err {
                break;
            }
        }
    });

    // 5. Main loop
    //    First iteration: initial prompt already sent → skip to response reading.
    //    Subsequent iterations: wait for user/pub-sub input, send, then read response.
    let mut first_turn = true;

    loop {
        if first_turn {
            first_turn = false;
        } else {
            if is_tty {
                eprint!("> ");
            }

            // Wait for input from any source (also watch for CLI exit)
            let input_result = tokio::select! {
                line = stdin_rx.recv() => {
                    match line {
                        Some(text) => {
                            let trimmed = text.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            // Exit commands
                            if matches!(trimmed, "exit" | "/exit" | "quit" | "/quit") {
                                Ok(false)
                            } else {
                                transport.send_message(&text).await
                                    .context("failed to send stdin input to CLI")?;
                                Ok(true)
                            }
                        }
                        None => Ok(false), // stdin closed (Ctrl-D)
                    }
                }
                input = async {
                    match pubsub_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match input {
                        Some(ps_input) => {
                            dispatch_input(&transport, ps_input).await
                                .context("failed to dispatch pub/sub input to CLI")?;
                            Ok(true)
                        }
                        None => Ok(false), // pub/sub channel closed
                    }
                }
                msg = cli_rx.recv() => {
                    // CLI process exited or sent data while waiting for input
                    match msg {
                        Some(Ok(None)) | None => Ok(false), // EOF / channel closed
                        Some(Ok(Some(ref m))) => {
                            render::render_message(m);
                            if let Some(ref ps) = pubsub {
                                if let Some(event) = cli_message_to_event(m) {
                                    ps.publish_event(&event).await.ok();
                                }
                            }
                            if matches!(m, CliMessage::Result(_)) {
                                Ok(false)
                            } else {
                                continue;
                            }
                        }
                        Some(Err(e)) => {
                            eprintln!("[error] CLI read error: {e}");
                            Ok(false)
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\n[info] Ctrl-C, exiting...");
                    Ok(false)
                }
                _ = async {
                    #[cfg(unix)]
                    sigterm.recv().await;
                    #[cfg(not(unix))]
                    std::future::pending::<()>().await;
                } => {
                    eprintln!("[info] SIGTERM received, shutting down...");
                    transport.send_control(crate::control::ControlRequest::interrupt()).await.ok();
                    if let Some(ref ps) = pubsub {
                        let event = crate::pubsub::PubSubEvent {
                            kind: crate::pubsub::PubSubKind::Error,
                            message: "Session terminated by SIGTERM".into(),
                            metadata: None,
                        };
                        ps.publish_event(&event).await.ok();
                    }
                    break;
                }
            };

            match input_result {
                Ok(true) => {}      // input dispatched, stream responses
                Ok(false) => break, // input source closed
                Err(e) => return Err(e),
            }
        }

        // 6. Stream responses until Result or EOF
        // (cancel-safe: mpsc recv never corrupts state when dropped)
        loop {
            let recv_result = tokio::select! {
                msg = cli_rx.recv() => {
                    match msg {
                        Some(result) => result,
                        None => Ok(None), // reader task exited
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    // Send interrupt to CLI (locks stdin Mutex — reader task only locks stdout)
                    transport.send_control(crate::control::ControlRequest::interrupt()).await.ok();
                    continue;
                }
            };

            match recv_result {
                Ok(Some(ref msg)) => {
                    render::render_message(msg);

                    // Publish to pub/sub
                    if let Some(ref ps) = pubsub {
                        if let Some(event) = cli_message_to_event(msg) {
                            ps.publish_event(&event).await.ok();
                        }
                    }

                    // Break on Result (completed or error)
                    if let CliMessage::Result(ref r) = msg {
                        render::notify_desktop(
                            "agent-runner",
                            if r.is_error {
                                "Agent completed with error"
                            } else {
                                "Agent completed"
                            },
                        );
                        break;
                    }
                }
                Ok(None) => {
                    // CLI exited
                    if let Some(ref ps) = pubsub {
                        let event = crate::pubsub::PubSubEvent {
                            kind: crate::pubsub::PubSubKind::Completed,
                            message: "CLI process exited".into(),
                            metadata: None,
                        };
                        ps.publish_event(&event).await.ok();
                    }
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("[error] CLI read error: {e}");
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::{BufReader, BufWriter};
    use tokio::sync::Mutex;

    use crate::pubsub::PubSubInput;

    /// Spawn `sh -c 'exec cat'` as a mock transport.
    async fn spawn_cat_transport() -> SubprocessTransport {
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
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        }
    }

    #[tokio::test]
    async fn init_timeout_triggers() {
        // Spawn a cat transport — it won't emit a System message
        let transport = spawn_cat_transport().await;
        let result = wait_for_init(&transport, 1).await;
        assert!(matches!(result, Err(CliError::InitTimeout(1))));
    }

    #[tokio::test]
    async fn init_succeeds_with_system_message() {
        let transport = spawn_cat_transport().await;

        // Write a system init message that cat echoes back
        let msg = r#"{"type":"system","subtype":"init","session_id":"test-init","model":"opus"}"#;
        {
            let mut stdin = transport.stdin.lock().await;
            tokio::io::AsyncWriteExt::write_all(&mut *stdin, format!("{msg}\n").as_bytes())
                .await
                .unwrap();
            tokio::io::AsyncWriteExt::flush(&mut *stdin).await.unwrap();
        }

        let sys = wait_for_init(&transport, 5).await.unwrap();
        assert_eq!(sys.session_id, "test-init");
        assert_eq!(sys.model.as_deref(), Some("opus"));
    }

    // R7: Test "CLI exited before init" path
    #[tokio::test]
    async fn init_eof_before_system_message() {
        // Spawn a process that immediately exits without writing anything
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "true"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let result = wait_for_init(&transport, 5).await;
        assert!(matches!(result, Err(CliError::SessionError(_))));
        if let Err(CliError::SessionError(msg)) = result {
            assert!(msg.contains("exited before init"));
        }
    }

    // R7: Test "wrong message type before init" path
    #[tokio::test]
    async fn init_wrong_message_type() {
        let transport = spawn_cat_transport().await;

        // Write an assistant message (not system) — should trigger "expected system init"
        let msg = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        {
            let mut stdin = transport.stdin.lock().await;
            tokio::io::AsyncWriteExt::write_all(&mut *stdin, format!("{msg}\n").as_bytes())
                .await
                .unwrap();
            tokio::io::AsyncWriteExt::flush(&mut *stdin).await.unwrap();
        }

        let result = wait_for_init(&transport, 5).await;
        assert!(matches!(result, Err(CliError::SessionError(_))));
        if let Err(CliError::SessionError(msg)) = result {
            assert!(msg.contains("expected system init"));
        }
    }

    #[tokio::test]
    async fn dispatch_prompt_input() {
        let transport = spawn_cat_transport().await;
        let input = PubSubInput::Prompt {
            content: "hello".into(),
            source: None,
        };
        dispatch_input(&transport, input).await.unwrap();

        // Verify the CLI received a user message
        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut *stdout, &mut line)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["content"], "hello");
    }

    #[tokio::test]
    async fn dispatch_control_interrupt() {
        let transport = spawn_cat_transport().await;
        let input = PubSubInput::Control {
            control: crate::control::ControlPayload::Interrupt,
        };
        dispatch_input(&transport, input).await.unwrap();

        // Verify interrupt was sent
        let mut stdout = transport.stdout.lock().await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut *stdout, &mut line)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "control");
        assert_eq!(parsed["control"]["type"], "interrupt");
    }

    #[tokio::test]
    async fn loop_exits_on_result_message() {
        // Spawn a process that emits a system init then a result message
        let mut child = tokio::process::Command::new("sh")
            .args([
                "-c",
                r#"echo '{"type":"system","subtype":"init","session_id":"s1"}'; echo '{"type":"result","subtype":"success","session_id":"s1","is_error":false}';"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        // wait_for_init consumes the system message
        let sys = wait_for_init(&transport, 5).await.unwrap();
        assert_eq!(sys.session_id, "s1");

        // Next recv should get the result message
        let msg = transport.recv().await.unwrap();
        assert!(matches!(msg, Some(CliMessage::Result(_))));
    }

    #[tokio::test]
    async fn loop_exits_on_eof() {
        // Spawn a process that emits system init then exits
        let mut child = tokio::process::Command::new("sh")
            .args([
                "-c",
                r#"echo '{"type":"system","subtype":"init","session_id":"s2"}'"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let _sys = wait_for_init(&transport, 5).await.unwrap();

        // Next recv should return None (EOF)
        let msg = transport.recv().await.unwrap();
        assert!(msg.is_none());
    }

    /// Test the full `run()` function with a mock process that emits init + result.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_completes_with_init_and_result() {
        // This process:
        // 1. Reads stdin (initial prompt), echoes it back (cat)
        // 2. Then emits system init + assistant + result
        let script = r#"
            read -r line
            echo '{"type":"system","subtype":"init","session_id":"run-test","model":"test"}'
            echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}'
            echo '{"type":"result","subtype":"success","session_id":"run-test","is_error":false,"result":"done"}'
        "#;
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(1);
        drop(tx); // stdin immediately closed
        let result = run(transport, None, "Hello agent".into(), stdin_rx).await;
        assert!(result.is_ok());
    }

    /// Test run() exits cleanly when the process exits after init (EOF path).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_exits_on_eof_after_init() {
        let script = r#"
            read -r line
            echo '{"type":"system","subtype":"init","session_id":"eof-test"}'
        "#;
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(1);
        drop(tx);
        let result = run(transport, None, "test".into(), stdin_rx).await;
        assert!(result.is_ok());
    }

    /// Test run() error when CLI exits before sending system init.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_fails_when_no_init() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "read -r line; exit 1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        let (tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(1);
        drop(tx);
        let result = run(transport, None, "test".into(), stdin_rx).await;
        assert!(result.is_err());
    }

    /// Test run() with error result from CLI.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_handles_error_result() {
        let script = r#"
            read -r line
            echo '{"type":"system","subtype":"init","session_id":"err-test"}'
            echo '{"type":"result","subtype":"error","session_id":"err-test","is_error":true,"result":"Rate limit"}'
        "#;
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let transport = SubprocessTransport {
            child,
            stdin: Mutex::new(BufWriter::new(stdin)),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        };

        // run() should complete without error even when CLI reports an error result
        let (tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(1);
        drop(tx);
        let result = run(transport, None, "test".into(), stdin_rx).await;
        assert!(result.is_ok());
    }
}
