// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Standalone K8s watcher binary entry point.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use clap::Parser;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use platform_k8s_watcher::config::WatcherConfig;
use platform_k8s_watcher::otlp::OtlpMetricsClient;
use platform_k8s_watcher::watcher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();

    let cfg = WatcherConfig::parse();
    let namespaces = cfg.namespaces();

    let kube_client = kube::Client::try_default().await?;
    let otlp_client = Arc::new(OtlpMetricsClient::new(
        cfg.ingest_endpoint.clone(),
        cfg.ingest_token.clone(),
    ));

    let cancel = CancellationToken::new();
    let tracker = TaskTracker::new();

    tracker.spawn(watcher::run(
        kube_client,
        otlp_client,
        namespaces,
        Duration::from_secs(cfg.flush_interval_secs),
        cancel.clone(),
    ));

    let app = Router::new().route("/healthz", get(|| async { "ok" }));

    tracing::info!(listen = %cfg.listen, "starting platform-k8s-watcher");

    let listener = TcpListener::bind(&cfg.listen).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(cancel.clone()))
        .await?;

    cancel.cancel();
    tracker.close();
    tracker.wait().await;

    tracing::info!("platform-k8s-watcher shutdown complete");
    Ok(())
}

async fn shutdown_signal(cancel: CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::select! {
        () = cancel.cancelled() => {}
        _ = ctrl_c => {
            tracing::info!("received SIGINT, shutting down");
        }
    }
}
