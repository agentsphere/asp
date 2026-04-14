// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Standalone OTLP ingest binary.
//!
//! Receives traces, logs, and metrics over HTTP protobuf, flushes to Postgres,
//! and publishes live-tail events to Valkey. Runs independently of the main
//! platform binary so telemetry keeps flowing during deploys/restarts.

mod auth;
mod config;
mod state;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Router};
use clap::Parser;
use prost::Message;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use platform_observe::ingest;
use platform_observe::proto;
use platform_types::ApiError;

use auth::IngestAuthUser;
use state::IngestState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default rustls crypto provider.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();

    let cfg = config::IngestConfig::parse();

    let pool = platform_types::pool::pg_connect(&cfg.database_url, 10, 30).await?;
    let valkey = platform_types::valkey::connect(&cfg.valkey_url, 4).await?;

    platform_auth::resolver::set_cache_ttl(cfg.permission_cache_ttl);

    let cancel = CancellationToken::new();
    let tracker = TaskTracker::new();

    // Build initial AlertRouter for the ingest tap
    let alert_router = match platform_observe::alert::AlertRouter::from_db(&pool).await {
        Ok(r) => std::sync::Arc::new(tokio::sync::RwLock::new(r)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to load alert router, starting empty");
            std::sync::Arc::new(tokio::sync::RwLock::new(
                platform_observe::alert::AlertRouter::empty(),
            ))
        }
    };

    // Alert rule subscriber — rebuilds router on rule changes
    tracker.spawn(platform_observe::alert::alert_rule_subscriber(
        pool.clone(),
        valkey.clone(),
        alert_router.clone(),
        cancel.clone(),
    ));

    let (channels, spans_rx, logs_rx, metrics_rx) =
        ingest::create_channels_with_capacity(cfg.buffer_capacity);

    tracker.spawn(ingest::flush_spans(pool.clone(), spans_rx, cancel.clone()));
    tracker.spawn(ingest::flush_logs(
        pool.clone(),
        valkey.clone(),
        logs_rx,
        cancel.clone(),
    ));
    tracker.spawn(ingest::flush_metrics(
        pool.clone(),
        valkey.clone(),
        alert_router,
        metrics_rx,
        cancel.clone(),
    ));

    let state = IngestState {
        pool,
        valkey,
        trust_proxy: cfg.trust_proxy,
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/traces", post(traces_handler))
        .route("/v1/logs", post(logs_handler))
        .route("/v1/metrics", post(metrics_handler))
        .layer(Extension(channels))
        .with_state(state);

    tracing::info!(listen = %cfg.listen, "starting platform-ingest");

    let listener = TcpListener::bind(&cfg.listen).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(cancel.clone()))
        .await?;

    cancel.cancel();
    tracker.close();
    tracker.wait().await;

    tracing::info!("platform-ingest shutdown complete");
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

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[tracing::instrument(skip(state, channels, headers, body))]
async fn traces_handler(
    State(state): State<IngestState>,
    IngestAuthUser(auth): IngestAuthUser,
    Extension(channels): Extension<ingest::IngestChannels>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let rate_id = auth
        .boundary_project_id
        .map_or_else(|| auth.user_id.to_string(), |pid| pid.to_string());
    platform_auth::check_rate(&state.valkey, "otlp", &rate_id, 10_000, 60).await?;

    let body = ingest::maybe_decompress(&headers, body)?;
    let request = proto::ExportTraceServiceRequest::decode(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid protobuf: {e}")))?;

    let checker = platform_auth::PgPermissionChecker {
        pool: &state.pool,
        valkey: &state.valkey,
    };
    let resource_attrs_refs: Vec<&[proto::KeyValue]> = request
        .resource_spans
        .iter()
        .map(|rs| rs.resource.as_ref().map_or(&[][..], |r| &r.attributes[..]))
        .collect();
    ingest::check_otlp_project_auth(&auth, &checker, &resource_attrs_refs).await?;

    for rs in &request.resource_spans {
        let resource_attrs = rs.resource.as_ref().map_or(&[][..], |r| &r.attributes);
        for ss in &rs.scope_spans {
            for span in &ss.spans {
                let record = ingest::build_span_record(span, resource_attrs, &state.pool).await;
                ingest::try_send_span(&channels, record)?;
            }
        }
    }

    let response_bytes = proto::ExportTraceServiceResponse {}.encode_to_vec();
    Ok((
        StatusCode::OK,
        [("content-type", "application/x-protobuf")],
        response_bytes,
    ))
}

#[tracing::instrument(skip(state, channels, headers, body))]
async fn logs_handler(
    State(state): State<IngestState>,
    IngestAuthUser(auth): IngestAuthUser,
    Extension(channels): Extension<ingest::IngestChannels>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let rate_id = auth
        .boundary_project_id
        .map_or_else(|| auth.user_id.to_string(), |pid| pid.to_string());
    platform_auth::check_rate(&state.valkey, "otlp", &rate_id, 10_000, 60).await?;

    let body = ingest::maybe_decompress(&headers, body)?;
    let request = proto::ExportLogsServiceRequest::decode(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid protobuf: {e}")))?;

    let checker = platform_auth::PgPermissionChecker {
        pool: &state.pool,
        valkey: &state.valkey,
    };
    let resource_attrs_refs: Vec<&[proto::KeyValue]> = request
        .resource_logs
        .iter()
        .map(|rl| rl.resource.as_ref().map_or(&[][..], |r| &r.attributes[..]))
        .collect();
    ingest::check_otlp_project_auth(&auth, &checker, &resource_attrs_refs).await?;

    for rl in &request.resource_logs {
        let resource_attrs = rl.resource.as_ref().map_or(&[][..], |r| &r.attributes);
        for sl in &rl.scope_logs {
            for log in &sl.log_records {
                let record = ingest::build_log_record(log, resource_attrs, &state.pool).await;
                ingest::try_send_log(&channels, record)?;
            }
        }
    }

    let response_bytes = proto::ExportLogsServiceResponse {}.encode_to_vec();
    Ok((
        StatusCode::OK,
        [("content-type", "application/x-protobuf")],
        response_bytes,
    ))
}

#[tracing::instrument(skip(state, channels, headers, body))]
async fn metrics_handler(
    State(state): State<IngestState>,
    IngestAuthUser(auth): IngestAuthUser,
    Extension(channels): Extension<ingest::IngestChannels>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let rate_id = auth
        .boundary_project_id
        .map_or_else(|| auth.user_id.to_string(), |pid| pid.to_string());
    platform_auth::check_rate(&state.valkey, "otlp", &rate_id, 10_000, 60).await?;

    let body = ingest::maybe_decompress(&headers, body)?;
    let request = proto::ExportMetricsServiceRequest::decode(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid protobuf: {e}")))?;

    let checker = platform_auth::PgPermissionChecker {
        pool: &state.pool,
        valkey: &state.valkey,
    };
    let resource_attrs_refs: Vec<&[proto::KeyValue]> = request
        .resource_metrics
        .iter()
        .map(|rm| rm.resource.as_ref().map_or(&[][..], |r| &r.attributes[..]))
        .collect();
    ingest::check_otlp_project_auth(&auth, &checker, &resource_attrs_refs).await?;

    for rm in &request.resource_metrics {
        let resource_attrs = rm.resource.as_ref().map_or(&[][..], |r| &r.attributes);
        for sm in &rm.scope_metrics {
            for metric in &sm.metrics {
                let records =
                    ingest::build_metric_records(metric, resource_attrs, &state.pool).await;
                for record in records {
                    ingest::try_send_metric(&channels, record)?;
                }
            }
        }
    }

    let response_bytes = proto::ExportMetricsServiceResponse {}.encode_to_vec();
    Ok((
        StatusCode::OK,
        [("content-type", "application/x-protobuf")],
        response_bytes,
    ))
}
