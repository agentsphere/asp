use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use super::anthropic::{self, ChatMessage};
use super::error::AgentError;
use super::provider::ProgressEvent;
use crate::store::AppState;

const BROADCAST_CAPACITY: usize = 256;

const CREATE_APP_SYSTEM_PROMPT: &str = "You are a helpful project planning assistant for the Platform developer tool. Your role is to help users clarify and refine their app ideas before implementation begins.

When a user describes what they want to build, you should:
1. Ask clarifying questions about the tech stack, architecture, and requirements
2. Discuss deployment preferences (language, framework, database, etc.)
3. Identify potential challenges or considerations
4. Summarize the final plan when the user is satisfied

Keep your responses concise and focused. Ask one or two questions at a time, not a long list.
When the user confirms the plan, provide a clear summary of what will be built.

You do NOT have access to any tools. You are a conversational assistant only.";

/// Handle for an in-process agent session.
/// Holds the broadcast channel and conversation history.
#[derive(Clone)]
pub struct InProcessHandle {
    pub tx: broadcast::Sender<ProgressEvent>,
    pub messages: Arc<RwLock<Vec<ChatMessage>>>,
    pub api_key: String,
    pub model: Option<String>,
}

impl InProcessHandle {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx,
            messages: Arc::new(RwLock::new(Vec::new())),
            api_key,
            model,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ProgressEvent> {
        self.tx.subscribe()
    }
}

/// Create an in-process session: resolve API key, store handle, set status to running,
/// and spawn the first conversation turn as a background task.
pub async fn create_inprocess_session(
    state: &AppState,
    user_id: Uuid,
    description: &str,
    provider_name: &str,
) -> Result<Uuid, AgentError> {
    let _ = super::service::get_provider(provider_name)?;

    // Resolve user API key
    let api_key = resolve_user_api_key(state, user_id).await.ok_or_else(|| {
        AgentError::Other(anyhow::anyhow!(
            "No Anthropic API key configured. Set your key in Settings > Provider Keys."
        ))
    })?;

    let session_id = Uuid::new_v4();

    // Insert DB row as 'running'
    sqlx::query(
        "INSERT INTO agent_sessions (id, user_id, prompt, provider, status) VALUES ($1, $2, $3, $4, 'running')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(description)
    .bind(provider_name)
    .execute(&state.pool)
    .await?;

    // Create handle
    let handle = InProcessHandle::new(api_key, None);

    // Store in AppState
    {
        let mut sessions = state.inprocess_sessions.write().unwrap();
        sessions.insert(session_id, handle.clone());
    }

    // Save first user message to DB
    sqlx::query("INSERT INTO agent_messages (session_id, role, content) VALUES ($1, 'user', $2)")
        .bind(session_id)
        .bind(description)
        .execute(&state.pool)
        .await?;

    // Add first user message to conversation history
    {
        let mut msgs = handle.messages.write().await;
        msgs.push(ChatMessage {
            role: "user".into(),
            content: description.into(),
        });
    }

    // Spawn first turn
    let state_clone = state.clone();
    let handle_clone = handle.clone();
    tokio::spawn(async move {
        if let Err(e) = run_turn(&state_clone, session_id, &handle_clone).await {
            tracing::error!(error = %e, %session_id, "first turn failed");
        }
    });

    Ok(session_id)
}

/// Send a follow-up user message to an in-process session.
pub async fn send_inprocess_message(
    state: &AppState,
    session_id: Uuid,
    content: &str,
) -> Result<(), AgentError> {
    let handle = {
        let sessions = state.inprocess_sessions.read().unwrap();
        sessions.get(&session_id).cloned()
    }
    .ok_or(AgentError::SessionNotRunning)?;

    // Save user message to DB
    sqlx::query("INSERT INTO agent_messages (session_id, role, content) VALUES ($1, 'user', $2)")
        .bind(session_id)
        .bind(content)
        .execute(&state.pool)
        .await?;

    // Add to conversation history
    {
        let mut msgs = handle.messages.write().await;
        msgs.push(ChatMessage {
            role: "user".into(),
            content: content.into(),
        });
    }

    // Spawn turn
    let state_clone = state.clone();
    let handle_clone = handle;
    let content_owned = content.to_owned();
    tokio::spawn(async move {
        let _ = &content_owned; // ensure lifetime
        if let Err(e) = run_turn(&state_clone, session_id, &handle_clone).await {
            tracing::error!(error = %e, %session_id, "turn failed");
        }
    });

    Ok(())
}

/// Run a single conversation turn: call Anthropic API, stream events, save response.
async fn run_turn(
    state: &AppState,
    session_id: Uuid,
    handle: &InProcessHandle,
) -> Result<(), anyhow::Error> {
    let messages = handle.messages.read().await.clone();

    let assistant_text = anthropic::stream_turn(
        &handle.api_key,
        handle.model.as_deref(),
        &messages,
        CREATE_APP_SYSTEM_PROMPT,
        &handle.tx,
    )
    .await?;

    // Save assistant response to DB
    if !assistant_text.is_empty() {
        sqlx::query(
            "INSERT INTO agent_messages (session_id, role, content) VALUES ($1, 'assistant', $2)",
        )
        .bind(session_id)
        .bind(&assistant_text)
        .execute(&state.pool)
        .await?;

        // Add to conversation history
        let mut msgs = handle.messages.write().await;
        msgs.push(ChatMessage {
            role: "assistant".into(),
            content: assistant_text,
        });
    }

    Ok(())
}

/// Get a broadcast receiver for an in-process session's events.
pub fn subscribe(state: &AppState, session_id: Uuid) -> Option<broadcast::Receiver<ProgressEvent>> {
    let sessions = state.inprocess_sessions.read().unwrap();
    sessions.get(&session_id).map(InProcessHandle::subscribe)
}

/// Remove an in-process session handle (called on stop/cleanup).
pub fn remove_session(state: &AppState, session_id: Uuid) {
    let mut sessions = state.inprocess_sessions.write().unwrap();
    sessions.remove(&session_id);
}

/// Try to resolve the user's Anthropic API key from `user_provider_keys`.
async fn resolve_user_api_key(state: &AppState, user_id: Uuid) -> Option<String> {
    let master_key_hex = state.config.master_key.as_deref()?;
    let master_key = crate::secrets::engine::parse_master_key(master_key_hex).ok()?;
    match crate::secrets::user_keys::get_user_key(&state.pool, &master_key, user_id, "anthropic")
        .await
    {
        Ok(key) => key,
        Err(e) => {
            tracing::warn!(error = %e, %user_id, "failed to resolve user API key");
            None
        }
    }
}
