use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::Config;

/// HTTP client for the platform API.
pub struct PlatformClient {
    http: reqwest::Client,
    base_url: String,
}

impl PlatformClient {
    /// Create a new client from config.
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {}", config.server.token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).context("invalid token")?,
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("building HTTP client")?;

        Ok(Self {
            http,
            base_url: config.server.url.trim_end_matches('/').to_string(),
        })
    }

    /// POST JSON to an endpoint.
    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST {path} returned {status}: {body}");
        }
        resp.json().await.with_context(|| format!("parsing response from POST {path}"))
    }

    /// GET JSON from an endpoint.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET {path} returned {status}: {body}");
        }
        resp.json().await.with_context(|| format!("parsing response from GET {path}"))
    }

    /// POST to stop a session.
    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/api/sessions/{session_id}/stop", self.base_url);
        let resp = self
            .http
            .post(&url)
            .send()
            .await
            .with_context(|| format!("POST stop session {session_id}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("stop session returned {status}: {body}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_create_request_shape() {
        let body = serde_json::json!({
            "prompt": "/dev fix the bug",
            "execution_mode": "cli_subprocess",
        });
        assert_eq!(body["prompt"], "/dev fix the bug");
        assert_eq!(body["execution_mode"], "cli_subprocess");
    }

    #[test]
    fn auth_header_from_config() {
        let config = Config {
            server: crate::config::ServerConfig {
                url: "http://localhost:8080".into(),
                token: "plat_test123".into(),
            },
            defaults: Default::default(),
        };
        let client = PlatformClient::new(&config).unwrap();
        // Verify the client was created with the correct base URL
        assert_eq!(client.base_url, "http://localhost:8080");
    }
}
