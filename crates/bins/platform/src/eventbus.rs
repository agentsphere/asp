// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Valkey-based event bus subscriber and dispatcher.
//!
//! Subscribes to `platform:events`, deserializes events, and dispatches
//! to domain crate handlers.

use fred::interfaces::PubsubInterface;
use fred::prelude::*;
use platform_types::events::{EVENTS_CHANNEL, PlatformEvent};
use tracing::Instrument;

use crate::state::PlatformState;

/// Background task: subscribe to platform events and dispatch to handlers.
pub async fn run(state: PlatformState, cancel: tokio_util::sync::CancellationToken) {
    tracing::info!("event bus subscriber started");

    let subscriber = state.valkey.next().clone_new();
    if let Err(e) = subscriber.init().await {
        tracing::error!(error = %e, "failed to init event bus subscriber");
        return;
    }

    if let Err(e) = subscriber.subscribe(EVENTS_CHANNEL).await {
        tracing::error!(error = %e, "failed to subscribe to {EVENTS_CHANNEL}");
        return;
    }

    let mut message_rx = subscriber.message_rx();
    state.task_registry.register("event_bus", 30);

    let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(25));
    keepalive.tick().await;

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tracing::info!("event bus subscriber shutting down");
                let _ = subscriber.unsubscribe(EVENTS_CHANNEL).await;
                break;
            }
            _ = keepalive.tick() => {
                state.task_registry.heartbeat("event_bus");
            }
            msg = message_rx.recv() => {
                match msg {
                    Ok(message) => {
                        state.task_registry.heartbeat("event_bus");
                        let payload: String = match message.value.convert() {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!(error = %e, "failed to convert message payload");
                                continue;
                            }
                        };
                        let state = state.clone();
                        let iter_trace_id = uuid::Uuid::new_v4().to_string().replace('-', "");
                        let span = tracing::info_span!(
                            "task_iteration",
                            task_name = "event_bus",
                            trace_id = %iter_trace_id,
                            source = "system",
                        );
                        tokio::spawn(async move {
                            if let Err(e) = handle_event(&state, &payload) {
                                tracing::error!(error = %e, "event handler failed");
                            }
                        }.instrument(span));
                    }
                    Err(e) => {
                        state.task_registry.report_error("event_bus", &e.to_string());
                        tracing::error!(error = %e, "event bus recv error");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

/// Dispatch a raw JSON event to domain handlers.
fn handle_event(state: &PlatformState, payload: &str) -> anyhow::Result<()> {
    let event: PlatformEvent = serde_json::from_str(payload)?;
    tracing::debug!(?event, "handling platform event");

    match event {
        // Deploy-related events wake the reconciler
        PlatformEvent::ReleaseCreated { .. }
        | PlatformEvent::ReleasePromoted { .. }
        | PlatformEvent::ReleaseRolledBack { .. }
        | PlatformEvent::TrafficShifted { .. }
        | PlatformEvent::OpsRepoUpdated { .. }
        | PlatformEvent::DeployRequested { .. }
        | PlatformEvent::RollbackRequested { .. } => {
            state.deploy_notify.notify_one();
        }

        // Pipeline events wake the executor
        PlatformEvent::PipelineQueued { .. } => {
            state.pipeline_notify.notify_one();
        }

        // Remaining events — no-op in event bus (handled by API/git layers or TODO)
        _ => {}
    }

    Ok(())
}
