#![allow(dead_code)]

//! Mock Anthropic Messages API server for integration testing.
//!
//! Speaks the real SSE protocol so that `anthropic::stream_turn_with_tools()`
//! exercises its full HTTP + streaming code path.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A content block in a mock response.
#[derive(Clone, Debug)]
pub enum ContentBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// A canned response the mock server will return for a single API call.
#[derive(Clone, Debug)]
pub struct MockResponse {
    pub content_blocks: Vec<ContentBlock>,
    pub stop_reason: String,
    pub status_code: u16,
    pub error_message: Option<String>,
}

impl MockResponse {
    /// Simple text-only response (stop_reason = "end_turn").
    pub fn text(text: &str) -> Self {
        Self {
            content_blocks: vec![ContentBlock::Text(text.into())],
            stop_reason: "end_turn".into(),
            status_code: 200,
            error_message: None,
        }
    }

    /// Response with tool_use blocks (stop_reason = "tool_use").
    pub fn tool_use(blocks: Vec<ContentBlock>) -> Self {
        Self {
            content_blocks: blocks,
            stop_reason: "tool_use".into(),
            status_code: 200,
            error_message: None,
        }
    }

    /// Response with text followed by tool_use blocks.
    pub fn text_then_tools(text: &str, tools: Vec<ContentBlock>) -> Self {
        let mut blocks = vec![ContentBlock::Text(text.into())];
        blocks.extend(tools);
        Self {
            content_blocks: blocks,
            stop_reason: "tool_use".into(),
            status_code: 200,
            error_message: None,
        }
    }

    /// Error response (non-200 status).
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            content_blocks: vec![],
            stop_reason: "end_turn".into(),
            status_code: status,
            error_message: Some(message.into()),
        }
    }
}

/// A captured incoming request for assertions.
#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub model: String,
    pub messages: serde_json::Value,
    pub tools: serde_json::Value,
    pub system: String,
    pub stream: bool,
    pub max_tokens: u64,
}

/// Shared state for the mock server.
#[derive(Clone)]
struct MockState {
    responses: Arc<Mutex<VecDeque<MockResponse>>>,
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
}

// ---------------------------------------------------------------------------
// MockAnthropicServer
// ---------------------------------------------------------------------------

pub struct MockAnthropicServer {
    addr: SocketAddr,
    responses: Arc<Mutex<VecDeque<MockResponse>>>,
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
}

impl MockAnthropicServer {
    /// Start a mock server on a random port. Returns immediately.
    pub async fn start() -> Self {
        let responses: Arc<Mutex<VecDeque<MockResponse>>> = Arc::new(Mutex::new(VecDeque::new()));
        let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));

        let state = MockState {
            responses: responses.clone(),
            captured: captured.clone(),
        };

        let app = Router::new()
            .route("/v1/messages", post(handle_messages))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        Self {
            addr,
            responses,
            captured,
        }
    }

    /// The base URL to pass as `api_url` (e.g. `http://127.0.0.1:12345/v1/messages`).
    pub fn url(&self) -> String {
        format!("http://{}/v1/messages", self.addr)
    }

    /// Enqueue a canned response for the next API call.
    pub async fn enqueue(&self, response: MockResponse) {
        self.responses.lock().await.push_back(response);
    }

    /// Return all captured requests so far.
    pub async fn requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }
}

// ---------------------------------------------------------------------------
// Axum handler
// ---------------------------------------------------------------------------

async fn handle_messages(
    AxumState(state): AxumState<MockState>,
    body: String,
) -> impl IntoResponse {
    // Parse and capture the request
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    {
        let captured = CapturedRequest {
            model: parsed
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            messages: parsed.get("messages").cloned().unwrap_or_default(),
            tools: parsed.get("tools").cloned().unwrap_or_default(),
            system: parsed
                .get("system")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            stream: parsed
                .get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            max_tokens: parsed
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        };
        state.captured.lock().await.push(captured);
    }

    // Dequeue next response
    let response = state
        .responses
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(|| MockResponse::error(500, "no mock response queued"));

    // Non-200: return error JSON
    if response.status_code != 200 {
        let error_body = serde_json::json!({
            "type": "error",
            "error": {
                "type": "rate_limit_error",
                "message": response.error_message.as_deref().unwrap_or("mock error"),
            }
        });
        return (
            StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            [("content-type", "application/json")],
            error_body.to_string(),
        )
            .into_response();
    }

    // Build SSE stream
    let sse = build_sse_response(&response, &parsed);

    (
        StatusCode::OK,
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        sse,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// SSE builder — follows Anthropic Messages API contract exactly
// ---------------------------------------------------------------------------

fn build_sse_response(resp: &MockResponse, request: &serde_json::Value) -> String {
    let model = request
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("claude-sonnet-4-5-20250929");

    let mut out = String::new();

    // message_start
    let msg_start = serde_json::json!({
        "type": "message_start",
        "message": {
            "id": "msg_mock_001",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": model,
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {"input_tokens": 100, "output_tokens": 1}
        }
    });
    write_event(&mut out, &msg_start);

    // ping
    write_event(&mut out, &serde_json::json!({"type": "ping"}));

    // Content blocks
    let mut index = 0u64;
    for block in &resp.content_blocks {
        match block {
            ContentBlock::Text(text) => {
                // content_block_start
                write_event(
                    &mut out,
                    &serde_json::json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "text", "text": ""}
                    }),
                );

                // Stream text in chunks (simulate real streaming)
                for chunk in text_chunks(text, 20) {
                    write_event(
                        &mut out,
                        &serde_json::json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": {"type": "text_delta", "text": chunk}
                        }),
                    );
                }

                // content_block_stop
                write_event(
                    &mut out,
                    &serde_json::json!({"type": "content_block_stop", "index": index}),
                );
            }
            ContentBlock::Thinking(thinking) => {
                write_event(
                    &mut out,
                    &serde_json::json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "thinking", "thinking": ""}
                    }),
                );

                write_event(
                    &mut out,
                    &serde_json::json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "thinking_delta", "thinking": thinking}
                    }),
                );

                write_event(
                    &mut out,
                    &serde_json::json!({"type": "content_block_stop", "index": index}),
                );
            }
            ContentBlock::ToolUse { id, name, input } => {
                // content_block_start with tool info
                write_event(
                    &mut out,
                    &serde_json::json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": {}
                        }
                    }),
                );

                // Stream input JSON in chunks (like real API)
                let input_str = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
                for chunk in json_chunks(&input_str, 15) {
                    write_event(
                        &mut out,
                        &serde_json::json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": {"type": "input_json_delta", "partial_json": chunk}
                        }),
                    );
                }

                // content_block_stop
                write_event(
                    &mut out,
                    &serde_json::json!({"type": "content_block_stop", "index": index}),
                );
            }
        }
        index += 1;
    }

    // message_delta
    write_event(
        &mut out,
        &serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": resp.stop_reason,
                "stop_sequence": null
            },
            "usage": {"output_tokens": 50}
        }),
    );

    // message_stop
    write_event(&mut out, &serde_json::json!({"type": "message_stop"}));

    out
}

fn write_event(out: &mut String, data: &serde_json::Value) {
    out.push_str("event: ");
    let event_type = data
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    out.push_str(event_type);
    out.push('\n');
    out.push_str("data: ");
    out.push_str(&data.to_string());
    out.push_str("\n\n");
}

/// Split text into chunks for realistic streaming.
fn text_chunks(text: &str, max_size: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    text.chars()
        .collect::<Vec<_>>()
        .chunks(max_size)
        .map(|c| c.iter().collect())
        .collect()
}

/// Split JSON string into chunks for partial_json streaming.
fn json_chunks(json: &str, max_size: usize) -> Vec<String> {
    if json.is_empty() {
        return vec!["{}".into()];
    }
    json.as_bytes()
        .chunks(max_size)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect()
}
