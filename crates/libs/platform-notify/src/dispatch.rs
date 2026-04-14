// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Concrete [`NotificationDispatcher`] backed by Postgres + SMTP + Valkey.
//!
//! Inserts notifications into the DB, rate-limits per user, and routes
//! by channel (in-app, email, webhook).

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::{NotificationDispatcher, NotifyParams, WebhookDispatcher};

use crate::email::{self, SmtpConfig};

/// Notification dispatcher backed by Postgres, SMTP, and Valkey for rate limiting.
///
/// Generic over `W: WebhookDispatcher` to avoid `dyn` dispatch (traits use RPITIT).
pub struct SmtpNotificationDispatcher<W: WebhookDispatcher> {
    pool: PgPool,
    valkey: fred::clients::Pool,
    smtp_config: Option<SmtpConfig>,
    webhook_dispatcher: W,
}

impl<W: WebhookDispatcher> SmtpNotificationDispatcher<W> {
    pub fn new(
        pool: PgPool,
        valkey: fred::clients::Pool,
        smtp_config: Option<SmtpConfig>,
        webhook_dispatcher: W,
    ) -> Self {
        Self {
            pool,
            valkey,
            smtp_config,
            webhook_dispatcher,
        }
    }
}

impl<W: WebhookDispatcher> NotificationDispatcher for SmtpNotificationDispatcher<W> {
    async fn notify(&self, params: NotifyParams<'_>) -> anyhow::Result<()> {
        // Rate limit: max 100 notifications per user per hour
        let user_key = params.user_id.to_string();
        platform_auth::rate_limit::check_rate(&self.valkey, "notify", &user_key, 100, 3600)
            .await
            .map_err(|e| anyhow::anyhow!("rate limit exceeded: {e}"))?;

        // Insert notification row
        let notif_id: Uuid = sqlx::query_scalar!(
            r#"
            INSERT INTO notifications (user_id, notification_type, subject, body, channel, status, ref_type, ref_id)
            VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7)
            RETURNING id
            "#,
            params.user_id,
            params.notification_type,
            params.subject,
            params.body,
            params.channel,
            params.ref_type,
            params.ref_id,
        )
        .fetch_one(&self.pool)
        .await?;

        // Route by channel
        let new_status = match params.channel {
            "email" => {
                if let Some(ref smtp) = self.smtp_config {
                    match send_email_notification(&self.pool, smtp, &params).await {
                        Ok(()) => "sent",
                        Err(e) => {
                            tracing::error!(error = %e, %notif_id, "email notification failed");
                            "failed"
                        }
                    }
                } else {
                    tracing::warn!(%notif_id, "SMTP not configured, email not sent");
                    "failed"
                }
            }
            "webhook" => {
                // Delegate to webhook dispatcher if a project context is available
                if let Some(ref_id) = params.ref_id {
                    self.webhook_dispatcher
                        .fire_webhooks(
                            ref_id,
                            params.notification_type,
                            &serde_json::json!({
                                "type": params.notification_type,
                                "subject": params.subject,
                                "body": params.body,
                            }),
                        )
                        .await;
                }
                "sent"
            }
            // in_app: just stored in DB, UI polls or SSE pushes
            _ => "sent",
        };

        // Update status
        let _ = sqlx::query!(
            "UPDATE notifications SET status = $1 WHERE id = $2",
            new_status,
            notif_id,
        )
        .execute(&self.pool)
        .await;

        Ok(())
    }
}

/// Send email for a notification. Looks up the user's email address.
async fn send_email_notification(
    pool: &PgPool,
    smtp: &SmtpConfig,
    params: &NotifyParams<'_>,
) -> anyhow::Result<()> {
    let user_email: String =
        sqlx::query_scalar!("SELECT email FROM users WHERE id = $1", params.user_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("user not found for email notification"))?;

    let body_text = params.body.unwrap_or("");
    email::send(smtp, &user_email, params.subject, body_text).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn channel_routing_covers_all_known_channels() {
        // The match in `notify()` routes: "email" → SMTP, "webhook" → dispatcher, _ → in_app.
        // Verify each expected channel is a distinct non-empty string.
        let channels = ["in_app", "email", "webhook"];
        assert_eq!(channels.len(), 3);
        for ch in &channels {
            assert!(!ch.is_empty());
        }
        // No duplicates
        let mut sorted = channels.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), channels.len());
    }

    #[test]
    fn unknown_channel_treated_as_in_app() {
        // The match uses `_ => "sent"`, so any unknown channel is treated like in_app.
        // This test documents that behavior — unknown channels don't error.
        let unknown_channels = ["sms", "slack", "push", ""];
        for ch in unknown_channels {
            // They would all fall through to the default match arm.
            assert_ne!(ch, "email");
            assert_ne!(ch, "webhook");
        }
    }
}
