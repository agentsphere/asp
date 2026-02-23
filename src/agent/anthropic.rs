use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::provider::{ProgressEvent, ProgressKind};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-5-20250929";
const MAX_TOKENS: u32 = 8192;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Stream a conversation turn via the Anthropic Messages API (SSE).
///
/// Returns a vector of `ProgressEvent`s and the full assistant text response.
/// Events are also sent through the provided `broadcast::Sender` in real-time.
pub async fn stream_turn(
    api_key: &str,
    model: Option<&str>,
    messages: &[ChatMessage],
    system: &str,
    tx: &tokio::sync::broadcast::Sender<ProgressEvent>,
) -> Result<String, anyhow::Error> {
    let model = model.unwrap_or(DEFAULT_MODEL);

    let body = serde_json::json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": system,
        "messages": messages,
    });

    let client = reqwest::Client::new();
    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let err_msg = format!("Anthropic API error ({status}): {body}");
        let _ = tx.send(ProgressEvent {
            kind: ProgressKind::Error,
            message: err_msg.clone(),
            metadata: None,
        });
        anyhow::bail!(err_msg);
    }

    let mut stream = response.bytes_stream();
    let mut full_text = String::new();
    let mut buf = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE lines
        while let Some(line_end) = buf.find('\n') {
            let line = buf[..line_end].trim_end_matches('\r').to_string();
            buf = buf[line_end + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                if let Some((event, text_delta)) = parse_sse_data(data) {
                    let _ = tx.send(event);
                    if let Some(delta) = text_delta {
                        full_text.push_str(&delta);
                    }
                }
            }
        }
    }

    // Signal completion
    let _ = tx.send(ProgressEvent {
        kind: ProgressKind::Completed,
        message: "Turn completed".into(),
        metadata: None,
    });

    Ok(full_text)
}

/// Parse a single SSE `data:` payload into a `ProgressEvent`.
/// Returns the event and optionally the text delta to accumulate.
fn parse_sse_data(data: &str) -> Option<(ProgressEvent, Option<String>)> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;

    match event_type {
        "content_block_delta" => {
            let delta = v.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;
            match delta_type {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_owned();
                    Some((
                        ProgressEvent {
                            kind: ProgressKind::Text,
                            message: text.clone(),
                            metadata: None,
                        },
                        Some(text),
                    ))
                }
                "thinking_delta" => {
                    let thinking = delta.get("thinking")?.as_str()?;
                    let truncated: String = thinking.chars().take(200).collect();
                    Some((
                        ProgressEvent {
                            kind: ProgressKind::Thinking,
                            message: truncated,
                            metadata: None,
                        },
                        None,
                    ))
                }
                _ => None,
            }
        }
        "message_start" | "content_block_start" | "content_block_stop" | "message_delta" => {
            // Informational events — skip
            None
        }
        // "message_stop" handled by wildcard — we send our own Completed event after the stream ends
        "error" => {
            let message = v
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown API error");
            Some((
                ProgressEvent {
                    kind: ProgressKind::Error,
                    message: message.to_owned(),
                    metadata: None,
                },
                None,
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let (event, delta) = parse_sse_data(data).unwrap();
        assert_eq!(event.kind, ProgressKind::Text);
        assert_eq!(event.message, "Hello");
        assert_eq!(delta.unwrap(), "Hello");
    }

    #[test]
    fn parse_thinking_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#;
        let (event, delta) = parse_sse_data(data).unwrap();
        assert_eq!(event.kind, ProgressKind::Thinking);
        assert_eq!(event.message, "Let me think...");
        assert!(delta.is_none());
    }

    #[test]
    fn parse_error_event() {
        let data = r#"{"type":"error","error":{"message":"overloaded"}}"#;
        let (event, _) = parse_sse_data(data).unwrap();
        assert_eq!(event.kind, ProgressKind::Error);
        assert_eq!(event.message, "overloaded");
    }

    #[test]
    fn parse_message_stop_returns_none() {
        let data = r#"{"type":"message_stop"}"#;
        assert!(parse_sse_data(data).is_none());
    }

    #[test]
    fn parse_message_start_returns_none() {
        let data = r#"{"type":"message_start","message":{"id":"msg_123"}}"#;
        assert!(parse_sse_data(data).is_none());
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_sse_data("not json").is_none());
        assert!(parse_sse_data("").is_none());
    }
}
