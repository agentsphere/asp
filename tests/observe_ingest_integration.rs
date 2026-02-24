//! Integration tests for the OTLP ingest pipeline (ingest → flush → query).

mod helpers;

use axum::Router;
use axum::http::StatusCode;
use prost::Message;
use sqlx::PgPool;

use helpers::{admin_login, test_state};

// ---------------------------------------------------------------------------
// Custom test router that includes ingest endpoints + channels
// ---------------------------------------------------------------------------

fn ingest_test_router(
    state: platform::store::AppState,
    channels: platform::observe::ingest::IngestChannels,
) -> Router {
    Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .merge(platform::api::router())
        .merge(platform::observe::router(channels))
        .merge(platform::registry::router())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal OTLP ExportTraceServiceRequest with one span.
fn build_trace_request(trace_id: &[u8; 16], span_id: &[u8; 8]) -> Vec<u8> {
    let request = platform::observe::proto::ExportTraceServiceRequest {
        resource_spans: vec![platform::observe::proto::ResourceSpans {
            resource: Some(platform::observe::proto::Resource {
                attributes: vec![platform::observe::proto::KeyValue {
                    key: "service.name".into(),
                    value: Some(platform::observe::proto::AnyValue {
                        value: Some(platform::observe::proto::any_value::Value::StringValue(
                            "ingest-test-svc".into(),
                        )),
                    }),
                }],
                ..Default::default()
            }),
            scope_spans: vec![platform::observe::proto::ScopeSpans {
                spans: vec![platform::observe::proto::Span {
                    trace_id: trace_id.to_vec(),
                    span_id: span_id.to_vec(),
                    name: "test-span".into(),
                    kind: 1, // SERVER
                    start_time_unix_nano: 1_700_000_000_000_000_000,
                    end_time_unix_nano: 1_700_000_000_050_000_000,
                    status: Some(platform::observe::proto::SpanStatus {
                        code: 1,
                        message: String::new(),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    request.encode_to_vec()
}

/// Build a minimal OTLP ExportLogsServiceRequest with one log record.
fn build_logs_request() -> Vec<u8> {
    let request = platform::observe::proto::ExportLogsServiceRequest {
        resource_logs: vec![platform::observe::proto::ResourceLogs {
            resource: Some(platform::observe::proto::Resource {
                attributes: vec![platform::observe::proto::KeyValue {
                    key: "service.name".into(),
                    value: Some(platform::observe::proto::AnyValue {
                        value: Some(platform::observe::proto::any_value::Value::StringValue(
                            "ingest-log-svc".into(),
                        )),
                    }),
                }],
                ..Default::default()
            }),
            scope_logs: vec![platform::observe::proto::ScopeLogs {
                log_records: vec![platform::observe::proto::LogRecord {
                    time_unix_nano: 1_700_000_000_000_000_000,
                    severity_number: 9, // INFO
                    severity_text: "INFO".into(),
                    body: Some(platform::observe::proto::AnyValue {
                        value: Some(platform::observe::proto::any_value::Value::StringValue(
                            "ingest test log message".into(),
                        )),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    request.encode_to_vec()
}

/// Build a minimal OTLP ExportMetricsServiceRequest with one gauge.
fn build_metrics_request(metric_name: &str) -> Vec<u8> {
    let request = platform::observe::proto::ExportMetricsServiceRequest {
        resource_metrics: vec![platform::observe::proto::ResourceMetrics {
            resource: Some(platform::observe::proto::Resource {
                attributes: vec![],
                ..Default::default()
            }),
            scope_metrics: vec![platform::observe::proto::ScopeMetrics {
                metrics: vec![platform::observe::proto::Metric {
                    name: metric_name.into(),
                    unit: "bytes".into(),
                    data: Some(platform::observe::proto::metric_data::Data::Gauge(
                        platform::observe::proto::Gauge {
                            data_points: vec![platform::observe::proto::NumberDataPoint {
                                value: Some(
                                    platform::observe::proto::number_data_point::Value::AsDouble(
                                        42.5,
                                    ),
                                ),
                                time_unix_nano: 1_700_000_000_000_000_000,
                                ..Default::default()
                            }],
                        },
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    request.encode_to_vec()
}

/// Send protobuf bytes to an ingest endpoint.
async fn post_protobuf(
    app: &Router,
    token: &str,
    path: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/x-protobuf")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body))
        .unwrap();

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app.clone(), req)
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Ingest a trace via OTLP protobuf, flush, query via API.
#[sqlx::test(migrations = "./migrations")]
async fn ingest_traces_protobuf(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    let (channels, spans_rx, _logs_rx, _metrics_rx) = platform::observe::ingest::create_channels();
    let app = ingest_test_router(state.clone(), channels);
    let admin_token = admin_login(&app).await;

    let trace_id: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let span_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let body = build_trace_request(&trace_id, &span_id);

    let (status, _) = post_protobuf(&app, &admin_token, "/v1/traces", body).await;
    assert_eq!(status, StatusCode::OK);

    // Manually flush by spawning a short-lived flush
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    let flush_pool = pool.clone();
    let handle = tokio::spawn(platform::observe::ingest::flush_spans(
        flush_pool,
        spans_rx,
        shutdown_rx,
    ));
    // Give the flush task a tick
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    // Verify via query
    let expected_trace_id = "0102030405060708090a0b0c0d0e0f10";
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/traces/{expected_trace_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "trace not found: {body}");
    assert_eq!(body["trace_id"], expected_trace_id);
}

/// Ingest logs via OTLP protobuf, flush, query.
#[sqlx::test(migrations = "./migrations")]
async fn ingest_logs_protobuf(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    let (channels, _spans_rx, logs_rx, _metrics_rx) = platform::observe::ingest::create_channels();
    let app = ingest_test_router(state.clone(), channels);
    let admin_token = admin_login(&app).await;

    let body = build_logs_request();
    let (status, _) = post_protobuf(&app, &admin_token, "/v1/logs", body).await;
    assert_eq!(status, StatusCode::OK);

    // Flush
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    let handle = tokio::spawn(platform::observe::ingest::flush_logs(
        pool.clone(),
        state.valkey.clone(),
        logs_rx,
        shutdown_rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    // Query
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        "/api/observe/logs?service=ingest-log-svc",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "logs query failed: {body}");
    assert!(body["total"].as_i64().unwrap() >= 1);
}

/// Ingest metrics via OTLP protobuf, flush, query.
#[sqlx::test(migrations = "./migrations")]
async fn ingest_metrics_protobuf(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    let (channels, _spans_rx, _logs_rx, metrics_rx) = platform::observe::ingest::create_channels();
    let app = ingest_test_router(state.clone(), channels);
    let admin_token = admin_login(&app).await;

    let metric_name = format!("ingest_test_{}", uuid::Uuid::new_v4().simple());
    let body = build_metrics_request(&metric_name);
    let (status, _) = post_protobuf(&app, &admin_token, "/v1/metrics", body).await;
    assert_eq!(status, StatusCode::OK);

    // Flush
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    let handle = tokio::spawn(platform::observe::ingest::flush_metrics(
        pool.clone(),
        metrics_rx,
        shutdown_rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    // Query
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/metrics?name={metric_name}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "metrics query failed: {body}");
    let series: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    assert!(!series.is_empty(), "metric series should exist");
}

/// Invalid protobuf returns 400.
#[sqlx::test(migrations = "./migrations")]
async fn ingest_invalid_protobuf_returns_400(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    let (channels, _spans_rx, _logs_rx, _metrics_rx) = platform::observe::ingest::create_channels();
    let app = ingest_test_router(state, channels);
    let admin_token = admin_login(&app).await;

    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0x00];
    let (status, _) = post_protobuf(&app, &admin_token, "/v1/traces", garbage.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post_protobuf(&app, &admin_token, "/v1/logs", garbage.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post_protobuf(&app, &admin_token, "/v1/metrics", garbage).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Flush drains channel on shutdown signal.
#[sqlx::test(migrations = "./migrations")]
async fn flush_shutdown_drains_remaining(pool: PgPool) {
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    // Send a span record
    let span = platform::observe::store::SpanRecord {
        trace_id: "drain-trace".into(),
        span_id: "drain-span".into(),
        parent_span_id: None,
        name: "drain-test".into(),
        service: "drain-svc".into(),
        kind: "server".into(),
        status: "ok".into(),
        attributes: None,
        events: None,
        duration_ms: Some(1),
        started_at: chrono::Utc::now(),
        finished_at: None,
        project_id: None,
        session_id: None,
        user_id: None,
    };
    tx.send(span).await.unwrap();
    drop(tx); // Close sender

    // Start flush with immediate shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    let flush_pool = pool.clone();
    let handle = tokio::spawn(platform::observe::ingest::flush_spans(
        flush_pool,
        rx,
        shutdown_rx,
    ));

    // Give first tick a chance, then signal shutdown
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    // Verify the span was written
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM spans WHERE service = 'drain-svc'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(count.0 >= 1, "span should have been flushed on shutdown");
}
