use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// CLI configuration loaded from `~/.platform-cli.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub token: String,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("url", &self.url)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub project: Option<String>,
    #[serde(default = "default_execution_mode")]
    pub execution_mode: String,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            project: None,
            execution_mode: default_execution_mode(),
        }
    }
}

fn default_execution_mode() -> String {
    "cli_subprocess".into()
}

impl Config {
    /// Load config from `~/.platform-cli.toml`.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
    }

    /// Load config from a specific path.
    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
    }

    /// WebSocket URL derived from the server HTTP URL.
    pub fn ws_url(&self) -> String {
        http_to_ws(&self.server.url)
    }
}

/// Default config file path: `~/.platform-cli.toml`.
pub fn config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".platform-cli.toml"))
}

/// Convert `http(s)://` URL to `ws(s)://`.
pub fn http_to_ws(url: &str) -> String {
    if url.starts_with("https://") {
        format!("wss://{}", &url[8..])
    } else if url.starts_with("http://") {
        format!("ws://{}", &url[7..])
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parse_toml() {
        let toml_str = r#"
[server]
url = "https://platform.example.com"
token = "plat_abc123"

[defaults]
project = "my-project"
execution_mode = "cli_subprocess"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.url, "https://platform.example.com");
        assert_eq!(config.server.token, "plat_abc123");
        assert_eq!(config.defaults.project.as_deref(), Some("my-project"));
        assert_eq!(config.defaults.execution_mode, "cli_subprocess");
    }

    #[test]
    fn config_parse_missing_defaults() {
        let toml_str = r#"
[server]
url = "https://example.com"
token = "tok"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.defaults.project.is_none());
        assert_eq!(config.defaults.execution_mode, "cli_subprocess");
    }

    #[test]
    fn config_parse_missing_server() {
        let toml_str = r#"
[defaults]
project = "p"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn ws_url_from_http() {
        assert_eq!(http_to_ws("http://localhost:8080"), "ws://localhost:8080");
    }

    #[test]
    fn ws_url_from_https() {
        assert_eq!(
            http_to_ws("https://platform.example.com"),
            "wss://platform.example.com"
        );
    }

    #[test]
    fn ws_url_no_scheme() {
        assert_eq!(http_to_ws("localhost:8080"), "localhost:8080");
    }
}
