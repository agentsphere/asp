use colored::Colorize;
use serde::Deserialize;

/// Progress event received from the platform WebSocket.
///
/// These types are duplicated from the server-side `ProgressEvent`/`ProgressKind`
/// intentionally — the client binary shares zero code with the server to avoid
/// coupling. Changes to server types require manual sync here.
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressEvent {
    pub kind: ProgressKind,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    Text,
    Thinking,
    ToolCall,
    ToolResult,
    Milestone,
    Error,
    Completed,
}

/// Render a progress event to the terminal with colored output.
pub fn render_event(event: &ProgressEvent) {
    match event.kind {
        ProgressKind::Thinking => {
            println!("{} {}", "thinking:".dimmed(), event.message.dimmed());
        }
        ProgressKind::Text => {
            println!("{}", event.message);
        }
        ProgressKind::ToolCall => {
            println!("{} {}", "tool:".cyan(), event.message);
        }
        ProgressKind::ToolResult => {
            println!("{} {}", "result:".blue(), event.message);
        }
        ProgressKind::Milestone => {
            println!("{} {}", "milestone:".green(), event.message.bold());
        }
        ProgressKind::Error => {
            eprintln!("{} {}", "error:".red().bold(), event.message);
        }
        ProgressKind::Completed => {
            println!("{} {}", "completed:".green().bold(), event.message);
            if let Some(ref meta) = event.metadata {
                if let Some(cost) = meta.get("total_cost_usd").and_then(|v| v.as_f64()) {
                    println!("  {} ${:.4}", "cost:".dimmed(), cost);
                }
                if let Some(turns) = meta.get("num_turns").and_then(|v| v.as_u64()) {
                    println!("  {} {}", "turns:".dimmed(), turns);
                }
                if let Some(ms) = meta.get("duration_ms").and_then(|v| v.as_u64()) {
                    let secs = ms as f64 / 1000.0;
                    println!("  {} {:.1}s", "duration:".dimmed(), secs);
                }
            }
        }
    }
}

/// Send a desktop notification (best-effort).
pub fn notify_desktop(title: &str, message: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    message.replace('"', "\\\""),
                    title.replace('"', "\\\""),
                ),
            ])
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([title, message])
            .output();
    }

    // Terminal bell on all platforms
    eprint!("\x07");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_thinking_event() {
        // Just verify it doesn't panic
        let event = ProgressEvent {
            kind: ProgressKind::Thinking,
            message: "Let me consider...".into(),
            metadata: None,
        };
        render_event(&event);
    }

    #[test]
    fn render_tool_call_event() {
        let event = ProgressEvent {
            kind: ProgressKind::ToolCall,
            message: "Read".into(),
            metadata: None,
        };
        render_event(&event);
    }

    #[test]
    fn render_completed_event() {
        let event = ProgressEvent {
            kind: ProgressKind::Completed,
            message: "Done.".into(),
            metadata: Some(serde_json::json!({
                "total_cost_usd": 0.05,
                "num_turns": 3,
                "duration_ms": 12345,
            })),
        };
        render_event(&event);
    }

    #[test]
    fn render_error_event() {
        let event = ProgressEvent {
            kind: ProgressKind::Error,
            message: "Rate limit exceeded".into(),
            metadata: None,
        };
        render_event(&event);
    }

    #[test]
    fn progress_kind_deserialize() {
        let json = r#"{"kind":"thinking","message":"hi","metadata":null}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.kind, ProgressKind::Thinking);
    }
}
