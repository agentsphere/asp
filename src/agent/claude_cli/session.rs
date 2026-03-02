use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};
use uuid::Uuid;

use super::error::CliError;
use super::messages::CliMessage;
use super::transport::SubprocessTransport;
use crate::agent::provider::{ProgressEvent, ProgressKind};

// ---------------------------------------------------------------------------
// Session mode (internal only — NOT persisted to DB)
// ---------------------------------------------------------------------------

/// How a CLI subprocess session behaves after receiving a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionMode {
    /// Send prompt, stream result, kill process.
    #[default]
    OneShot,
    /// Keep process alive for multi-turn conversation.
    Persistent,
}

// ---------------------------------------------------------------------------
// Session handle
// ---------------------------------------------------------------------------

/// Internal state for an active CLI subprocess session.
pub struct CliSessionHandle {
    pub transport: Arc<Mutex<SubprocessTransport>>,
    pub tx: broadcast::Sender<ProgressEvent>,
    pub mode: SessionMode,
    pub session_id: Uuid,
    pub cli_session_id: Mutex<Option<String>>,
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

/// Manages active CLI subprocess sessions running in the platform pod.
#[derive(Clone)]
pub struct CliSessionManager {
    sessions: Arc<RwLock<HashMap<Uuid, Arc<CliSessionHandle>>>>,
    max_concurrent: usize,
}

impl CliSessionManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_concurrent,
        }
    }

    /// Register a new CLI subprocess session. Returns error if concurrent limit is reached.
    pub async fn register(
        &self,
        session_id: Uuid,
        transport: SubprocessTransport,
        mode: SessionMode,
    ) -> Result<broadcast::Receiver<ProgressEvent>, CliError> {
        let mut sessions = self.sessions.write().await;
        if sessions.len() >= self.max_concurrent {
            return Err(CliError::SessionError(format!(
                "concurrent CLI subprocess limit reached (max {})",
                self.max_concurrent
            )));
        }

        let (tx, rx) = broadcast::channel(256);
        let handle = Arc::new(CliSessionHandle {
            transport: Arc::new(Mutex::new(transport)),
            tx,
            mode,
            session_id,
            cli_session_id: Mutex::new(None),
        });

        sessions.insert(session_id, handle);
        Ok(rx)
    }

    /// Subscribe to progress events for an active session.
    pub async fn subscribe(
        &self,
        session_id: Uuid,
    ) -> Result<broadcast::Receiver<ProgressEvent>, CliError> {
        let sessions = self.sessions.read().await;
        let handle = sessions
            .get(&session_id)
            .ok_or_else(|| CliError::SessionError("CLI session not found".into()))?;
        Ok(handle.tx.subscribe())
    }

    /// Get a reference to the session handle.
    pub async fn get(&self, session_id: Uuid) -> Option<Arc<CliSessionHandle>> {
        self.sessions.read().await.get(&session_id).cloned()
    }

    /// Remove a session from the manager.
    pub async fn remove(&self, session_id: Uuid) -> Option<Arc<CliSessionHandle>> {
        self.sessions.write().await.remove(&session_id)
    }

    /// Current number of active sessions.
    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Maximum concurrent sessions allowed.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }
}

// ---------------------------------------------------------------------------
// CliMessage → ProgressEvent conversion
// ---------------------------------------------------------------------------

/// Convert a CLI NDJSON message to a platform `ProgressEvent`.
///
/// Returns `None` for message types that don't map to progress events.
pub fn cli_message_to_progress(msg: &CliMessage) -> Option<ProgressEvent> {
    match msg {
        CliMessage::System(sys) => Some(ProgressEvent {
            kind: ProgressKind::Milestone,
            message: format!(
                "Session started (model: {})",
                sys.model.as_deref().unwrap_or("default")
            ),
            metadata: Some(serde_json::json!({
                "session_id": sys.session_id,
                "claude_code_version": sys.claude_code_version,
            })),
        }),
        CliMessage::Assistant(a) => convert_assistant_progress(a),
        CliMessage::User(u) => convert_user_progress(u),
        CliMessage::Result(r) => Some(convert_result_progress(r)),
    }
}

/// Extract progress from assistant message content blocks.
fn convert_assistant_progress(a: &super::messages::AssistantMessage) -> Option<ProgressEvent> {
    let content = &a.message.content;
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t.to_owned());
                }
            }
            Some("thinking") => {
                if let Some(t) = block.get("thinking").and_then(|v| v.as_str()) {
                    return Some(ProgressEvent {
                        kind: ProgressKind::Thinking,
                        message: t.to_owned(),
                        metadata: None,
                    });
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tool_calls.push(name.to_owned());
            }
            _ => {}
        }
    }

    if !tool_calls.is_empty() {
        Some(ProgressEvent {
            kind: ProgressKind::ToolCall,
            message: tool_calls.join(", "),
            metadata: None,
        })
    } else if !text_parts.is_empty() {
        Some(ProgressEvent {
            kind: ProgressKind::Text,
            message: text_parts.join(""),
            metadata: None,
        })
    } else {
        None
    }
}

/// Extract progress from user message content blocks (tool results).
fn convert_user_progress(u: &super::messages::UserMessage) -> Option<ProgressEvent> {
    let content = &u.message.content;
    let mut results = Vec::new();

    for block in content {
        if let Some("tool_result") = block.get("type").and_then(|t| t.as_str()) {
            let tool_id = block
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            results.push(tool_id.to_owned());
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(ProgressEvent {
            kind: ProgressKind::ToolResult,
            message: format!("Tool results: {}", results.join(", ")),
            metadata: None,
        })
    }
}

/// Convert a result message to a completed/error progress event.
fn convert_result_progress(r: &super::messages::ResultMessage) -> ProgressEvent {
    let message = if r.is_error {
        r.result
            .as_deref()
            .unwrap_or("Agent completed with error")
            .to_owned()
    } else {
        r.result
            .as_deref()
            .unwrap_or("Agent completed successfully")
            .to_owned()
    };

    let kind = if r.is_error {
        ProgressKind::Error
    } else {
        ProgressKind::Completed
    };

    ProgressEvent {
        kind,
        message,
        metadata: Some(serde_json::json!({
            "total_cost_usd": r.total_cost_usd,
            "duration_ms": r.duration_ms,
            "num_turns": r.num_turns,
            "is_error": r.is_error,
        })),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_mode_default_oneshot() {
        assert_eq!(SessionMode::default(), SessionMode::OneShot);
    }

    #[tokio::test]
    async fn cli_session_manager_new() {
        let mgr = CliSessionManager::new(10);
        assert_eq!(mgr.active_count().await, 0);
        assert_eq!(mgr.max_concurrent(), 10);
    }

    #[tokio::test]
    async fn subscribe_unknown_session_returns_error() {
        let mgr = CliSessionManager::new(10);
        let result = mgr.subscribe(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn remove_session_decrements_count() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();

        // We need a real transport, but for testing we can construct one
        // using the spawn_cat helper. Instead, test the manager's map directly.
        // Register a mock by inserting directly.
        {
            let (tx, _rx) = broadcast::channel(16);
            let handle = Arc::new(CliSessionHandle {
                transport: Arc::new(Mutex::new(spawn_dummy_transport().await)),
                tx,
                mode: SessionMode::OneShot,
                session_id: sid,
                cli_session_id: Mutex::new(None),
            });
            mgr.sessions.write().await.insert(sid, handle);
        }

        assert_eq!(mgr.active_count().await, 1);
        mgr.remove(sid).await;
        assert_eq!(mgr.active_count().await, 0);
    }

    #[tokio::test]
    async fn concurrent_limit_enforced() {
        let mgr = CliSessionManager::new(2);

        // Register 2 sessions (at limit)
        for _ in 0..2 {
            let (tx, _rx) = broadcast::channel(16);
            let handle = Arc::new(CliSessionHandle {
                transport: Arc::new(Mutex::new(spawn_dummy_transport().await)),
                tx,
                mode: SessionMode::OneShot,
                session_id: Uuid::new_v4(),
                cli_session_id: Mutex::new(None),
            });
            mgr.sessions.write().await.insert(handle.session_id, handle);
        }

        // 3rd should fail
        let result = mgr
            .register(
                Uuid::new_v4(),
                spawn_dummy_transport().await,
                SessionMode::OneShot,
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("limit reached"));
    }

    // -- cli_message_to_progress tests --

    #[test]
    fn cli_message_to_progress_assistant_text() {
        let msg = CliMessage::Assistant(crate::agent::claude_cli::messages::AssistantMessage {
            message: crate::agent::claude_cli::messages::AssistantContent {
                content: vec![serde_json::json!({"type": "text", "text": "Hello world"})],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Text);
        assert_eq!(event.message, "Hello world");
    }

    #[test]
    fn cli_message_to_progress_assistant_thinking() {
        let msg = CliMessage::Assistant(crate::agent::claude_cli::messages::AssistantMessage {
            message: crate::agent::claude_cli::messages::AssistantContent {
                content: vec![
                    serde_json::json!({"type": "thinking", "thinking": "Let me consider..."}),
                ],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Thinking);
        assert!(event.message.contains("Let me consider"));
    }

    #[test]
    fn cli_message_to_progress_tool_call() {
        let msg = CliMessage::Assistant(crate::agent::claude_cli::messages::AssistantMessage {
            message: crate::agent::claude_cli::messages::AssistantContent {
                content: vec![
                    serde_json::json!({"type": "tool_use", "name": "Read", "id": "t1", "input": {}}),
                ],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::ToolCall);
        assert!(event.message.contains("Read"));
    }

    #[test]
    fn cli_message_to_progress_tool_result() {
        let msg = CliMessage::User(crate::agent::claude_cli::messages::UserMessage {
            message: crate::agent::claude_cli::messages::UserContent {
                content: vec![
                    serde_json::json!({"type": "tool_result", "tool_use_id": "t1", "content": "file contents"}),
                ],
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::ToolResult);
    }

    #[test]
    fn cli_message_to_progress_result_success() {
        let msg = CliMessage::Result(crate::agent::claude_cli::messages::ResultMessage {
            subtype: "success".into(),
            session_id: "s1".into(),
            is_error: false,
            result: Some("Done.".into()),
            total_cost_usd: Some(0.05),
            duration_ms: Some(1234),
            num_turns: Some(3),
            usage: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Completed);
        assert_eq!(event.message, "Done.");
    }

    #[test]
    fn cli_message_to_progress_result_error() {
        let msg = CliMessage::Result(crate::agent::claude_cli::messages::ResultMessage {
            subtype: "error".into(),
            session_id: "s1".into(),
            is_error: true,
            result: Some("Rate limit exceeded".into()),
            total_cost_usd: None,
            duration_ms: None,
            num_turns: None,
            usage: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Error);
        assert!(event.message.contains("Rate limit"));
    }

    /// Helper: create a dummy SubprocessTransport for manager tests.
    async fn spawn_dummy_transport() -> SubprocessTransport {
        use std::process::Stdio;
        use tokio::io::{BufReader, BufWriter};

        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "exec cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cat");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        SubprocessTransport {
            child,
            stdin: tokio::sync::Mutex::new(BufWriter::new(stdin)),
            stdout: tokio::sync::Mutex::new(BufReader::new(stdout)),
            stderr_task: None,
            session_id: tokio::sync::Mutex::new(None),
            alive: std::sync::atomic::AtomicBool::new(true),
        }
    }
}
