// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! SMTP email sending.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// SMTP configuration for sending emails.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub from: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Send a plain-text email via SMTP.
///
/// Sanitizes `to` and `subject` to prevent header injection. Retries once
/// on transient failure.
#[tracing::instrument(skip(config, body), fields(%to), err)]
pub async fn send(config: &SmtpConfig, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
    // Email header injection prevention
    if to.contains('\n') || to.contains('\r') {
        anyhow::bail!("email 'to' address contains invalid characters");
    }
    if subject.contains('\n') || subject.contains('\r') {
        anyhow::bail!("email subject contains invalid characters");
    }

    let from: Mailbox = config
        .from
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid smtp from address '{}': {e}", config.from))?;

    let to_mailbox: Mailbox = to
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid recipient address '{to}': {e}"))?;

    let message = Message::builder()
        .from(from)
        .to(to_mailbox)
        .subject(subject)
        .body(body.to_owned())
        .map_err(|e| anyhow::anyhow!("failed to build email: {e}"))?;

    let mut transport = if config.port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
            .port(465)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
            .port(config.port)
    };

    if let Some(ref username) = config.username {
        let password = config.password.as_deref().unwrap_or("");
        transport = transport.credentials(Credentials::new(username.clone(), password.to_owned()));
    }

    let transport = transport.build();

    // One retry on transient failure
    match transport.send(message.clone()).await {
        Ok(_) => {
            tracing::info!(to, subject, "email sent");
            Ok(())
        }
        Err(first_err) => {
            tracing::warn!(error = %first_err, "email send failed, retrying once");
            transport
                .send(message)
                .await
                .map_err(|e| anyhow::anyhow!("email send failed after retry: {e}"))?;
            tracing::info!(to, subject, "email sent on retry");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SmtpConfig {
        SmtpConfig {
            host: "localhost".into(),
            port: 587,
            from: "noreply@example.com".into(),
            username: None,
            password: None,
        }
    }

    #[tokio::test]
    async fn reject_newline_in_to() {
        let config = test_config();
        let result = send(
            &config,
            "user@example.com\nBcc: evil@attacker.com",
            "test",
            "body",
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reject_newline_in_subject() {
        let config = test_config();
        let result = send(&config, "user@example.com", "test\r\nBcc: evil", "body").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reject_cr_in_to() {
        let config = test_config();
        let result = send(&config, "user@example.com\rBcc: evil", "test", "body").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn invalid_from_returns_error() {
        let mut config = test_config();
        config.from = "not-an-email".into();
        let result = send(&config, "user@example.com", "test", "body").await;
        assert!(result.is_err());
    }
}
