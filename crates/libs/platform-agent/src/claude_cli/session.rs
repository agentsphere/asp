// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::error::CliError;
use super::messages::CliMessage;
use crate::provider::{ProgressEvent, ProgressKind};

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
///
/// Progress events are published via Valkey pub/sub (not broadcast channel).
/// `pending_messages` queues user messages received while the tool loop is busy.
#[derive(Debug)]
pub struct CliSessionHandle {
    pub mode: SessionMode,
    pub session_id: Uuid,
    pub cli_session_id: Mutex<Option<String>>,
    /// Cancellation flag — checked between tool rounds in the create-app loop.
    pub cancelled: AtomicBool,
    /// Queued user messages — drained between tool rounds or after tool loop finishes.
    pub pending_messages: Mutex<Vec<String>>,
    /// Whether the tool loop is currently running (prevents concurrent invocations).
    pub busy: AtomicBool,
    /// User who owns this session (for tool execution context).
    pub user_id: Uuid,
}

impl CliSessionHandle {
    /// Check whether the tool loop is currently running.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
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
        user_id: Uuid,
        mode: SessionMode,
    ) -> Result<Arc<CliSessionHandle>, CliError> {
        let mut sessions = self.sessions.write().await;
        if sessions.len() >= self.max_concurrent {
            return Err(CliError::SessionError(format!(
                "concurrent CLI subprocess limit reached (max {})",
                self.max_concurrent
            )));
        }

        let handle = Arc::new(CliSessionHandle {
            mode,
            session_id,
            cli_session_id: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            pending_messages: Mutex::new(Vec::new()),
            busy: AtomicBool::new(false),
            user_id,
        });

        sessions.insert(session_id, handle.clone());
        Ok(handle)
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
    async fn register_and_get_session() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();
        let uid = Uuid::new_v4();
        let handle = mgr
            .register(sid, uid, SessionMode::Persistent)
            .await
            .unwrap();
        assert_eq!(handle.session_id, sid);
        assert_eq!(handle.user_id, uid);
        assert!(!handle.is_busy());

        let got = mgr.get(sid).await.unwrap();
        assert_eq!(got.session_id, sid);
    }

    #[tokio::test]
    async fn remove_session_decrements_count() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();
        mgr.register(sid, Uuid::new_v4(), SessionMode::OneShot)
            .await
            .unwrap();

        assert_eq!(mgr.active_count().await, 1);
        mgr.remove(sid).await;
        assert_eq!(mgr.active_count().await, 0);
    }

    #[tokio::test]
    async fn concurrent_limit_enforced() {
        let mgr = CliSessionManager::new(2);

        mgr.register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await
            .unwrap();
        mgr.register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await
            .unwrap();

        // 3rd should fail
        let result = mgr
            .register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("limit reached"));
    }

    #[tokio::test]
    async fn cancelled_flag_works() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();
        let handle = mgr
            .register(sid, Uuid::new_v4(), SessionMode::Persistent)
            .await
            .unwrap();

        assert!(!handle.cancelled.load(Ordering::Relaxed));
        handle.cancelled.store(true, Ordering::Relaxed);
        assert!(handle.cancelled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn pending_messages_queue() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();
        let handle = mgr
            .register(sid, Uuid::new_v4(), SessionMode::Persistent)
            .await
            .unwrap();

        handle.pending_messages.lock().await.push("msg1".into());
        handle.pending_messages.lock().await.push("msg2".into());

        let drained: Vec<String> = handle.pending_messages.lock().await.drain(..).collect();
        assert_eq!(drained, vec!["msg1", "msg2"]);
        assert!(handle.pending_messages.lock().await.is_empty());
    }

    #[tokio::test]
    async fn busy_flag_works() {
        let mgr = CliSessionManager::new(10);
        let sid = Uuid::new_v4();
        let handle = mgr
            .register(sid, Uuid::new_v4(), SessionMode::Persistent)
            .await
            .unwrap();

        assert!(!handle.is_busy());
        handle.busy.store(true, Ordering::Relaxed);
        assert!(handle.is_busy());
    }

    // -- cli_message_to_progress tests --

    #[test]
    fn cli_message_to_progress_assistant_text() {
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
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
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
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
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
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
        let msg = CliMessage::User(crate::claude_cli::messages::UserMessage {
            message: crate::claude_cli::messages::UserContent {
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
        let msg = CliMessage::Result(crate::claude_cli::messages::ResultMessage {
            subtype: "success".into(),
            session_id: "s1".into(),
            is_error: false,
            result: Some("Done.".into()),
            total_cost_usd: Some(0.05),
            duration_ms: Some(1234),
            num_turns: Some(3),
            usage: None,
            structured_output: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Completed);
        assert_eq!(event.message, "Done.");
    }

    #[test]
    fn cli_message_to_progress_result_error() {
        let msg = CliMessage::Result(crate::claude_cli::messages::ResultMessage {
            subtype: "error".into(),
            session_id: "s1".into(),
            is_error: true,
            result: Some("Rate limit exceeded".into()),
            total_cost_usd: None,
            duration_ms: None,
            num_turns: None,
            usage: None,
            structured_output: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Error);
        assert!(event.message.contains("Rate limit"));
    }

    // -- Additional cli_message_to_progress edge cases --

    #[test]
    fn cli_message_to_progress_system_with_model() {
        let msg = CliMessage::System(crate::claude_cli::messages::SystemMessage {
            subtype: "init".into(),
            session_id: "sess-123".into(),
            model: Some("claude-opus-4-20250514".into()),
            tools: Some(vec!["Read".into(), "Write".into()]),
            claude_code_version: Some("2.1.0".into()),
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Milestone);
        assert!(event.message.contains("claude-opus-4-20250514"));
        let meta = event.metadata.unwrap();
        assert_eq!(meta["session_id"], "sess-123");
        assert_eq!(meta["claude_code_version"], "2.1.0");
    }

    #[test]
    fn cli_message_to_progress_system_no_model() {
        let msg = CliMessage::System(crate::claude_cli::messages::SystemMessage {
            subtype: "init".into(),
            session_id: "sess-abc".into(),
            model: None,
            tools: None,
            claude_code_version: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert!(event.message.contains("default"));
    }

    #[test]
    fn cli_message_to_progress_assistant_empty_content() {
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
                content: vec![],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg);
        assert!(event.is_none(), "empty content should produce no event");
    }

    #[test]
    fn cli_message_to_progress_assistant_multiple_tool_calls() {
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
                content: vec![
                    serde_json::json!({"type": "tool_use", "name": "Read", "id": "t1", "input": {}}),
                    serde_json::json!({"type": "tool_use", "name": "Write", "id": "t2", "input": {}}),
                ],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::ToolCall);
        assert!(event.message.contains("Read"));
        assert!(event.message.contains("Write"));
        assert!(event.message.contains(", "));
    }

    #[test]
    fn cli_message_to_progress_assistant_unknown_block_type() {
        let msg = CliMessage::Assistant(crate::claude_cli::messages::AssistantMessage {
            message: crate::claude_cli::messages::AssistantContent {
                content: vec![serde_json::json!({"type": "unknown_type", "data": "something"})],
                model: None,
                usage: None,
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg);
        assert!(
            event.is_none(),
            "unknown block type should produce no event"
        );
    }

    #[test]
    fn cli_message_to_progress_user_empty_content() {
        let msg = CliMessage::User(crate::claude_cli::messages::UserMessage {
            message: crate::claude_cli::messages::UserContent { content: vec![] },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg);
        assert!(
            event.is_none(),
            "empty user content should produce no event"
        );
    }

    #[test]
    fn cli_message_to_progress_user_non_tool_result() {
        let msg = CliMessage::User(crate::claude_cli::messages::UserMessage {
            message: crate::claude_cli::messages::UserContent {
                content: vec![serde_json::json!({"type": "text", "text": "user message"})],
            },
            session_id: None,
        });
        let event = cli_message_to_progress(&msg);
        assert!(
            event.is_none(),
            "non-tool_result user content should produce no event"
        );
    }

    #[test]
    fn cli_message_to_progress_result_success_no_result_text() {
        let msg = CliMessage::Result(crate::claude_cli::messages::ResultMessage {
            subtype: "success".into(),
            session_id: "s1".into(),
            is_error: false,
            result: None,
            total_cost_usd: None,
            duration_ms: None,
            num_turns: None,
            usage: None,
            structured_output: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Completed);
        assert_eq!(event.message, "Agent completed successfully");
    }

    #[test]
    fn cli_message_to_progress_result_error_no_result_text() {
        let msg = CliMessage::Result(crate::claude_cli::messages::ResultMessage {
            subtype: "error".into(),
            session_id: "s1".into(),
            is_error: true,
            result: None,
            total_cost_usd: None,
            duration_ms: None,
            num_turns: None,
            usage: None,
            structured_output: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        assert_eq!(event.kind, ProgressKind::Error);
        assert_eq!(event.message, "Agent completed with error");
    }

    #[test]
    fn cli_message_to_progress_result_metadata_includes_cost() {
        let msg = CliMessage::Result(crate::claude_cli::messages::ResultMessage {
            subtype: "success".into(),
            session_id: "s1".into(),
            is_error: false,
            result: Some("done".into()),
            total_cost_usd: Some(0.05),
            duration_ms: Some(1234),
            num_turns: Some(3),
            usage: None,
            structured_output: None,
        });
        let event = cli_message_to_progress(&msg).unwrap();
        let meta = event.metadata.unwrap();
        assert_eq!(meta["total_cost_usd"], 0.05);
        assert_eq!(meta["duration_ms"], 1234);
        assert_eq!(meta["num_turns"], 3);
        assert_eq!(meta["is_error"], false);
    }

    // -- Session mode --

    #[test]
    fn session_mode_debug() {
        let s = format!("{:?}", SessionMode::Persistent);
        assert!(s.contains("Persistent"));
        let s = format!("{:?}", SessionMode::OneShot);
        assert!(s.contains("OneShot"));
    }

    #[test]
    fn session_mode_clone() {
        let mode = SessionMode::Persistent;
        let cloned = mode;
        assert_eq!(cloned, SessionMode::Persistent);
    }

    #[test]
    fn session_mode_eq() {
        assert_eq!(SessionMode::OneShot, SessionMode::OneShot);
        assert_eq!(SessionMode::Persistent, SessionMode::Persistent);
        assert_ne!(SessionMode::OneShot, SessionMode::Persistent);
    }

    // -- CliSessionHandle get nonexistent --

    #[tokio::test]
    async fn get_nonexistent_session_returns_none() {
        let mgr = CliSessionManager::new(10);
        assert!(mgr.get(Uuid::new_v4()).await.is_none());
    }

    #[tokio::test]
    async fn remove_nonexistent_session_returns_none() {
        let mgr = CliSessionManager::new(10);
        assert!(mgr.remove(Uuid::new_v4()).await.is_none());
    }

    // -- Concurrent limit edge cases --

    #[tokio::test]
    async fn concurrent_limit_zero_rejects_all() {
        let mgr = CliSessionManager::new(0);
        let result = mgr
            .register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn concurrent_limit_one_allows_one() {
        let mgr = CliSessionManager::new(1);
        let result = mgr
            .register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await;
        assert!(result.is_ok());
        let result2 = mgr
            .register(Uuid::new_v4(), Uuid::new_v4(), SessionMode::OneShot)
            .await;
        assert!(result2.is_err());
    }
}
