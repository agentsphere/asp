mod client;
mod commands;
mod config;
mod stream;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use client::PlatformClient;
use config::Config;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "platform-cli", about = "Remote Claude agent session manager")]
struct Cli {
    /// Config file path (default: ~/.platform-cli.toml)
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage sessions
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Shorthand: create a persistent /dev session
    Dev {
        /// The prompt/task description
        prompt: String,
        /// Target project name or ID
        #[arg(long)]
        project: Option<String>,
    },
    /// Shorthand: create a one-shot /plan session
    Plan {
        prompt: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Shorthand: create a one-shot /review session
    Review {
        #[arg(long)]
        project: Option<String>,
    },
    /// Upload Claude CLI credentials to the platform
    UploadCreds {
        /// Auth type: "setup_token" or "oauth"
        #[arg(long, default_value = "setup_token")]
        auth_type: String,
        /// The token value (or path to .credentials.json for oauth)
        token: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Create a new session
    Create {
        prompt: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "cli_subprocess")]
        execution: String,
        /// Attach to output stream after creation
        #[arg(long)]
        attach: bool,
    },
    /// List sessions
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Attach to a running session's WebSocket stream
    Attach {
        session_id: String,
    },
    /// Stop a running session
    Stop {
        session_id: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = if let Some(ref path) = cli.config {
        Config::load_from(path)?
    } else {
        Config::load().context(
            "failed to load config from ~/.platform-cli.toml\n\
             Create the file with:\n\
             [server]\n\
             url = \"https://your-platform.example.com\"\n\
             token = \"plat_your_api_token\"",
        )?
    };

    let api = PlatformClient::new(&config)?;

    match cli.command {
        Commands::Dev { prompt, project } => {
            let expanded = commands::expand_shorthand("dev", &prompt)
                .expect("dev is a known shorthand");
            create_and_attach(&config, &api, &expanded.prompt, project.as_deref()).await?;
        }
        Commands::Plan { prompt, project } => {
            let expanded = commands::expand_shorthand("plan", &prompt)
                .expect("plan is a known shorthand");
            create_and_attach(&config, &api, &expanded.prompt, project.as_deref()).await?;
        }
        Commands::Review { project } => {
            let expanded = commands::expand_shorthand("review", "")
                .expect("review is a known shorthand");
            create_and_attach(&config, &api, &expanded.prompt, project.as_deref()).await?;
        }
        Commands::UploadCreds { auth_type, token } => {
            upload_credentials(&api, &auth_type, &token).await?;
        }
        Commands::Session { action } => match action {
            SessionAction::Create {
                prompt,
                project,
                execution,
                attach,
            } => {
                let session_id = create_session(&api, &prompt, project.as_deref(), &execution).await?;
                if attach {
                    attach_session(&config, &session_id).await?;
                }
            }
            SessionAction::List { project, status } => {
                list_sessions(&api, project.as_deref(), status.as_deref()).await?;
            }
            SessionAction::Attach { session_id } => {
                attach_session(&config, &session_id).await?;
            }
            SessionAction::Stop { session_id } => {
                api.stop_session(&session_id).await?;
                println!("{} Session {} stopped", "ok:".green(), session_id);
            }
        },
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

async fn create_session(
    api: &PlatformClient,
    prompt: &str,
    project: Option<&str>,
    execution_mode: &str,
) -> Result<String> {
    let mut body = serde_json::json!({
        "prompt": prompt,
        "execution_mode": execution_mode,
    });
    if let Some(p) = project {
        body["project"] = serde_json::Value::String(p.into());
    }

    let resp: serde_json::Value = api.post("/api/sessions/cli", &body).await?;
    let session_id = resp["id"]
        .as_str()
        .context("missing session id in response")?;

    println!(
        "{} Session {} created",
        "ok:".green(),
        session_id.bold()
    );

    Ok(session_id.to_owned())
}

async fn create_and_attach(
    config: &Config,
    api: &PlatformClient,
    prompt: &str,
    project: Option<&str>,
) -> Result<()> {
    let execution_mode = &config.defaults.execution_mode;
    let session_id = create_session(api, prompt, project, execution_mode).await?;
    attach_session(config, &session_id).await
}

async fn attach_session(config: &Config, session_id: &str) -> Result<()> {
    let ws_base = config.ws_url();
    let ws_url = format!(
        "{ws_base}/api/sessions/{session_id}/ws?token={}",
        config.server.token
    );

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("WebSocket connection failed")?;

    let (mut write, mut read) = ws_stream.split();

    // Spawn stdin reader for interactive input
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);
        let mut lines = tokio::io::AsyncBufReadExt::lines(reader);
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).await.is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(event) = serde_json::from_str::<stream::ProgressEvent>(&text) {
                            stream::render_event(&event);
                            if event.kind == stream::ProgressKind::Completed
                                || event.kind == stream::ProgressKind::Error
                            {
                                stream::notify_desktop("Platform CLI", &event.message);
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            line = rx.recv() => {
                match line {
                    Some(input) => {
                        let msg = serde_json::json!({"content": input});
                        write.send(Message::Text(msg.to_string().into())).await?;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

async fn list_sessions(
    api: &PlatformClient,
    project: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let path = match (project, status) {
        (Some(p), Some(s)) => format!("/api/sessions?project={p}&status={s}"),
        (Some(p), None) => format!("/api/sessions?project={p}"),
        (None, Some(s)) => format!("/api/sessions?status={s}"),
        (None, None) => "/api/sessions".into(),
    };
    let resp: serde_json::Value = api.get(&path).await?;

    let items = resp["items"].as_array().context("missing items")?;
    if items.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    for item in items {
        let id = item["id"].as_str().unwrap_or("?");
        let status = item["status"].as_str().unwrap_or("?");
        let prompt = item["prompt"].as_str().unwrap_or("").chars().take(60).collect::<String>();
        let mode = item["execution_mode"].as_str().unwrap_or("?");

        let status_colored = match status {
            "running" => status.yellow().to_string(),
            "completed" => status.green().to_string(),
            "stopped" | "failed" => status.red().to_string(),
            _ => status.dimmed().to_string(),
        };

        println!(
            "{} {} [{}] {}",
            id.get(..8).unwrap_or(id),
            status_colored,
            mode.dimmed(),
            prompt,
        );
    }

    Ok(())
}

async fn upload_credentials(
    api: &PlatformClient,
    auth_type: &str,
    token: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "auth_type": auth_type,
        "token": token,
    });

    let _resp: serde_json::Value = api.post("/api/auth/cli-credentials", &body).await?;
    println!(
        "{} CLI credentials stored (type: {})",
        "ok:".green(),
        auth_type
    );
    Ok(())
}
