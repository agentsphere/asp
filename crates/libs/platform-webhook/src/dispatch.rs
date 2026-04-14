// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Concrete [`WebhookDispatcher`] implementation backed by Postgres + reqwest.
//!
//! Queries the `webhooks` table for matching active webhooks, then spawns
//! a tokio task for each delivery with concurrency limiting and HMAC signing.

use std::sync::{Arc, LazyLock};

use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use platform_types::WebhookDispatcher;

use crate::signing;
use crate::ssrf;

/// Maximum concurrent webhook deliveries.
const MAX_CONCURRENT_DELIVERIES: usize = 50;

/// Shared HTTP client for webhook deliveries.
static WEBHOOK_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to build webhook HTTP client")
});

/// Webhook dispatcher backed by Postgres + reqwest.
///
/// Looks up matching webhooks in the DB, spawns delivery tasks with
/// concurrency limiting, and signs payloads with HMAC-SHA256 when configured.
#[derive(Clone)]
pub struct WebhookDispatch {
    pool: PgPool,
    semaphore: Arc<Semaphore>,
}

impl WebhookDispatch {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DELIVERIES)),
        }
    }
}

impl WebhookDispatcher for WebhookDispatch {
    async fn fire_webhooks(&self, project_id: Uuid, event_name: &str, payload: &serde_json::Value) {
        let webhooks = match sqlx::query!(
            r#"
            SELECT id, url, secret
            FROM webhooks
            WHERE project_id = $1 AND active = true AND $2 = ANY(events)
            "#,
            project_id,
            event_name,
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(error = %e, %project_id, event_name, "failed to query webhooks");
                return;
            }
        };

        for wh in webhooks {
            let webhook_id = wh.id;
            let url = wh.url.clone();
            let secret = wh.secret.clone();
            let payload = payload.clone();
            let sem = self.semaphore.clone();

            tokio::spawn(async move {
                dispatch_single(webhook_id, &url, secret.as_deref(), &payload, &sem).await;
            });
        }
    }
}

/// Deliver a single webhook with SSRF check, HMAC signing, and concurrency control.
async fn dispatch_single(
    webhook_id: Uuid,
    url: &str,
    secret: Option<&str>,
    payload: &serde_json::Value,
    semaphore: &Semaphore,
) {
    // SSRF re-validation before dispatch
    if ssrf::validate_webhook_url(url).is_err() {
        tracing::warn!(%webhook_id, "webhook URL failed SSRF validation, skipping");
        return;
    }

    // Concurrency limit
    let Ok(_permit) = semaphore.try_acquire() else {
        tracing::warn!(%webhook_id, "webhook dispatch dropped: concurrency limit reached");
        return;
    };

    let body = match serde_json::to_string(payload) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, %webhook_id, "failed to serialize webhook payload");
            return;
        }
    };

    let mut request = WEBHOOK_CLIENT
        .post(url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "Platform-Webhook/1.0");

    // HMAC-SHA256 signing
    if let Some(secret) = secret
        && let Some(sig) = signing::sign_payload(secret, body.as_bytes())
    {
        request = request.header("X-Platform-Signature", sig);
    }

    match request.body(body).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                tracing::warn!(%webhook_id, %status, "webhook delivery got non-2xx response");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, %webhook_id, "webhook delivery failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_dispatch_is_constructible() {
        // Verify the struct layout. Actual dispatch is tested in integration tests.
        let _ctor = |pool: PgPool| WebhookDispatch::new(pool);
    }

    #[test]
    fn max_concurrent_deliveries() {
        assert_eq!(MAX_CONCURRENT_DELIVERIES, 50);
    }
}
