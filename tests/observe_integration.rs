//! Integration tests for the observability module (traces, logs, metrics, alerts).

mod helpers;

use axum::http::StatusCode;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use helpers::{admin_login, create_user, test_router, test_state};

// ---------------------------------------------------------------------------
// Store-level helpers
// ---------------------------------------------------------------------------

/// Insert a span via the store layer, then verify it's queryable via the HTTP API.
async fn insert_test_span(pool: &PgPool, trace_id: &str, span_id: &str, service: &str) {
    let now = Utc::now();
    let span = platform::observe::store::SpanRecord {
        trace_id: trace_id.into(),
        span_id: span_id.into(),
        parent_span_id: None,
        name: "test-span".into(),
        service: service.into(),
        kind: "server".into(),
        status: "ok".into(),
        attributes: None,
        events: None,
        duration_ms: Some(42),
        started_at: now,
        finished_at: Some(now + chrono::Duration::milliseconds(42)),
        project_id: None,
        session_id: None,
        user_id: None,
    };
    platform::observe::store::write_spans(pool, &[span])
        .await
        .expect("write_spans failed");
}

async fn insert_test_log(pool: &PgPool, service: &str, level: &str, message: &str) {
    let log = platform::observe::store::LogEntryRecord {
        timestamp: Utc::now(),
        trace_id: None,
        span_id: None,
        project_id: None,
        session_id: None,
        user_id: None,
        service: service.into(),
        level: level.into(),
        message: message.into(),
        attributes: None,
    };
    platform::observe::store::write_logs(pool, &[log])
        .await
        .expect("write_logs failed");
}

async fn insert_test_metric(pool: &PgPool, name: &str, value: f64) {
    let metric = platform::observe::store::MetricRecord {
        name: name.into(),
        labels: serde_json::json!({"host": "test-node"}),
        metric_type: "gauge".into(),
        unit: Some("bytes".into()),
        project_id: None,
        timestamp: Utc::now(),
        value,
    };
    platform::observe::store::write_metrics(pool, &[metric])
        .await
        .expect("write_metrics failed");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Write spans via store, query via /api/observe/traces → span appears.
#[sqlx::test(migrations = "./migrations")]
async fn write_and_query_spans(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let trace_id = format!("trace-{}", Uuid::new_v4());
    let span_id = format!("span-{}", Uuid::new_v4());
    insert_test_span(&pool, &trace_id, &span_id, "obs-test-svc").await;

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/observe/traces").await;

    assert_eq!(status, StatusCode::OK, "trace query failed: {body}");
    let items = body["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|t| t["trace_id"].as_str() == Some(&trace_id)),
        "trace not found in results: {body}"
    );
}

/// Write logs via store, query via /api/observe/logs → log appears.
#[sqlx::test(migrations = "./migrations")]
async fn write_and_query_logs(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let unique_msg = format!("test-log-{}", Uuid::new_v4());
    insert_test_log(&pool, "log-test-svc", "info", &unique_msg).await;

    // unique_msg is alphanumeric + hyphens, safe to pass as query param directly
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/logs?q={unique_msg}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "log query failed: {body}");
    assert!(
        body["total"].as_i64().unwrap() >= 1,
        "log not found: {body}"
    );
}

/// Write metrics via store, query via /api/observe/metrics → metric series appears.
#[sqlx::test(migrations = "./migrations")]
async fn write_and_query_metrics(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let metric_name = format!("test_metric_{}", Uuid::new_v4().simple());
    insert_test_metric(&pool, &metric_name, 42.0).await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/metrics?name={metric_name}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "metric query failed: {body}");
    let series: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    assert!(!series.is_empty(), "metric series empty");
    assert_eq!(series[0]["name"], metric_name);
}

/// List distinct metric names.
#[sqlx::test(migrations = "./migrations")]
async fn list_metric_names(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let name1 = format!("mn_alpha_{}", Uuid::new_v4().simple());
    let name2 = format!("mn_beta_{}", Uuid::new_v4().simple());
    insert_test_metric(&pool, &name1, 1.0).await;
    insert_test_metric(&pool, &name2, 2.0).await;

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/observe/metrics/names").await;

    assert_eq!(status, StatusCode::OK, "names query failed: {body}");
    let names: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    let name_strs: Vec<&str> = names.iter().filter_map(|n| n["name"].as_str()).collect();
    assert!(name_strs.contains(&name1.as_str()), "name1 missing");
    assert!(name_strs.contains(&name2.as_str()), "name2 missing");
}

/// Alert CRUD cycle: create, get, list, patch, delete.
#[sqlx::test(migrations = "./migrations")]
async fn alert_crud(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    // Need alert:manage permission — admin has it
    // Create
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/observe/alerts",
        serde_json::json!({
            "name": "high-cpu",
            "query": "metric:cpu_usage agg:avg window:300",
            "condition": "gt",
            "threshold": 90.0,
            "severity": "critical",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "alert create failed: {body}");
    let alert_id = body["id"].as_str().unwrap();

    // Get
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "high-cpu");

    // List
    let (status, body) = helpers::get_json(&app, &admin_token, "/api/observe/alerts").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["total"].as_i64().unwrap() >= 1);

    // Patch
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}"),
        serde_json::json!({ "name": "very-high-cpu", "threshold": 95.0 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "alert patch failed: {body}");
    assert_eq!(body["name"], "very-high-cpu");

    // Delete
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify gone
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// User without observe:read gets 403 on query endpoints.
#[sqlx::test(migrations = "./migrations")]
async fn observe_requires_permission(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    // Create user with NO role at all — no observe:read
    let (_user_id, token) = create_user(&app, &admin_token, "no-observe", "noobs@test.com").await;

    let (status, _) = helpers::get_json(&app, &token, "/api/observe/traces").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = helpers::get_json(&app, &token, "/api/observe/logs").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Log query filter tests
// ---------------------------------------------------------------------------

/// Filter logs by level — only error logs returned when ?level=error.
#[sqlx::test(migrations = "./migrations")]
async fn search_logs_by_level(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let tag = Uuid::new_v4().simple().to_string();
    insert_test_log(&pool, &format!("svc-{tag}"), "info", "info msg").await;
    insert_test_log(&pool, &format!("svc-{tag}"), "error", "error msg").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/logs?service=svc-{tag}&level=error"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["level"], "error");
}

/// Filter logs by service.
#[sqlx::test(migrations = "./migrations")]
async fn search_logs_by_service(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let svc_a = format!("svc-a-{}", Uuid::new_v4().simple());
    let svc_b = format!("svc-b-{}", Uuid::new_v4().simple());
    insert_test_log(&pool, &svc_a, "info", "msg from a").await;
    insert_test_log(&pool, &svc_b, "info", "msg from b").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/logs?service={svc_a}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert!(items.iter().all(|i| i["service"].as_str() == Some(&svc_a)));
}

/// Filter logs by text query.
#[sqlx::test(migrations = "./migrations")]
async fn search_logs_by_text_query(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let unique = Uuid::new_v4().simple().to_string();
    insert_test_log(&pool, "search-svc", "info", &format!("findme-{unique}")).await;
    insert_test_log(&pool, "search-svc", "info", "not matching").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/logs?q=findme-{unique}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_i64().unwrap(), 1);
}

/// Log pagination works.
#[sqlx::test(migrations = "./migrations")]
async fn search_logs_pagination(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let svc = format!("page-svc-{}", Uuid::new_v4().simple());
    for i in 0..5 {
        insert_test_log(&pool, &svc, "info", &format!("log {i}")).await;
    }

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/logs?service={svc}&limit=2&offset=0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert!(body["total"].as_i64().unwrap() >= 5);
}

// ---------------------------------------------------------------------------
// Trace detail tests
// ---------------------------------------------------------------------------

/// Get trace detail returns spans.
#[sqlx::test(migrations = "./migrations")]
async fn get_trace_detail(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let trace_id = format!("trace-detail-{}", Uuid::new_v4().simple());
    insert_test_span(&pool, &trace_id, "span-root", "detail-svc").await;
    insert_test_span(&pool, &trace_id, "span-child1", "detail-svc").await;
    insert_test_span(&pool, &trace_id, "span-child2", "detail-svc").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/traces/{trace_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "trace detail failed: {body}");
    assert_eq!(body["trace_id"], trace_id);
    assert_eq!(body["spans"].as_array().unwrap().len(), 3);
}

/// Get trace detail for nonexistent trace returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_trace_not_found(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        "/api/observe/traces/nonexistent-trace-id-12345",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Metric query tests
// ---------------------------------------------------------------------------

/// Query metrics by name.
#[sqlx::test(migrations = "./migrations")]
async fn query_metrics_by_name(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let name = format!("qm_{}", Uuid::new_v4().simple());
    insert_test_metric(&pool, &name, 99.0).await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/metrics?name={name}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let series: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    assert_eq!(series.len(), 1);
    assert!(!series[0]["points"].as_array().unwrap().is_empty());
}

/// Query metrics with time range filter.
#[sqlx::test(migrations = "./migrations")]
async fn query_metrics_time_range(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let name = format!("tr_{}", Uuid::new_v4().simple());
    insert_test_metric(&pool, &name, 50.0).await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/metrics?name={name}&range=1h"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let series: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    assert!(!series.is_empty());
}

/// List metric names returns distinct entries.
#[sqlx::test(migrations = "./migrations")]
async fn list_metric_names_returns_distinct(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let name = format!("dup_{}", Uuid::new_v4().simple());
    // Insert same metric name twice
    insert_test_metric(&pool, &name, 1.0).await;
    insert_test_metric(&pool, &name, 2.0).await;

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/observe/metrics/names").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<serde_json::Value> = serde_json::from_value(body).unwrap();
    let matching: Vec<_> = names
        .iter()
        .filter(|n| n["name"].as_str() == Some(&name))
        .collect();
    assert_eq!(matching.len(), 1, "metric name should appear exactly once");
}

// ---------------------------------------------------------------------------
// Alert handler tests
// ---------------------------------------------------------------------------

/// Create alert with missing fields returns 400.
#[sqlx::test(migrations = "./migrations")]
async fn create_alert_validation(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    // Missing required "query" field
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/observe/alerts",
        serde_json::json!({ "name": "bad-alert", "condition": "gt", "threshold": 1.0, "severity": "warning" }),
    )
    .await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "expected 400 or 422, got {status}"
    );
}

/// Partial update preserves other fields.
#[sqlx::test(migrations = "./migrations")]
async fn update_alert_partial(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/observe/alerts",
        serde_json::json!({
            "name": "partial-test",
            "query": "metric:mem agg:avg window:60",
            "condition": "gt",
            "threshold": 80.0,
            "severity": "warning",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let alert_id = body["id"].as_str().unwrap();

    // Patch only enabled=false
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}"),
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["name"], "partial-test"); // preserved
    assert_eq!(body["severity"], "warning"); // preserved
}

/// List alert events for a new alert returns empty.
#[sqlx::test(migrations = "./migrations")]
async fn list_alert_events_empty(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/observe/alerts",
        serde_json::json!({
            "name": "no-events",
            "query": "metric:disk agg:max window:60",
            "condition": "gt",
            "threshold": 90.0,
            "severity": "info",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let alert_id = body["id"].as_str().unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/observe/alerts/{alert_id}/events"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

/// List all alert events endpoint returns empty when no events.
#[sqlx::test(migrations = "./migrations")]
async fn list_all_alert_events(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/observe/alerts/events").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].as_array().is_some());
}

// ---------------------------------------------------------------------------
// Permission tests
// ---------------------------------------------------------------------------

/// Non-admin user cannot create alerts.
#[sqlx::test(migrations = "./migrations")]
async fn alert_manage_requires_permission(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (_uid, token) = create_user(&app, &admin_token, "no-alert", "noalert@test.com").await;

    let (status, _) = helpers::post_json(
        &app,
        &token,
        "/api/observe/alerts",
        serde_json::json!({
            "name": "unauthorized",
            "query": "metric:x agg:avg window:60",
            "condition": "gt",
            "threshold": 1.0,
            "severity": "info",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Non-admin user gets 403 on log endpoint.
#[sqlx::test(migrations = "./migrations")]
async fn observe_logs_requires_permission(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (_uid, token) = create_user(&app, &admin_token, "no-observe2", "noobs2@test.com").await;

    let (status, _) = helpers::get_json(&app, &token, "/api/observe/logs").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Parquet rotation tests
// ---------------------------------------------------------------------------

/// Rotate old logs to parquet — rows deleted from DB, bytes in MinIO.
#[sqlx::test(migrations = "./migrations")]
async fn rotate_logs_archives_old_data(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    // Insert log with timestamp 72h ago (> 48h cutoff)
    let old_ts = Utc::now() - chrono::Duration::hours(72);
    let log = platform::observe::store::LogEntryRecord {
        timestamp: old_ts,
        trace_id: None,
        span_id: None,
        project_id: None,
        session_id: None,
        user_id: None,
        service: "rotate-svc".into(),
        level: "info".into(),
        message: "old log for rotation".into(),
        attributes: None,
    };
    platform::observe::store::write_logs(&pool, &[log])
        .await
        .unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM log_entries WHERE service = 'rotate-svc'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(count.0 >= 1, "log should exist before rotation");

    // Run rotation
    let rotated = platform::observe::parquet::rotate_logs(&state)
        .await
        .unwrap();
    assert!(rotated >= 1, "should have rotated at least 1 log");

    // Verify deleted from DB
    let count_after: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM log_entries WHERE service = 'rotate-svc'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_after.0, 0, "rotated logs should be deleted");
}

/// Rotate old spans to parquet.
#[sqlx::test(migrations = "./migrations")]
async fn rotate_spans_archives_old_data(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    let old_ts = Utc::now() - chrono::Duration::hours(72);
    let span = platform::observe::store::SpanRecord {
        trace_id: "rot-trace".into(),
        span_id: "rot-span".into(),
        parent_span_id: None,
        name: "rotate-test".into(),
        service: "rotate-span-svc".into(),
        kind: "server".into(),
        status: "ok".into(),
        attributes: None,
        events: None,
        duration_ms: Some(10),
        started_at: old_ts,
        finished_at: Some(old_ts + chrono::Duration::milliseconds(10)),
        project_id: None,
        session_id: None,
        user_id: None,
    };
    platform::observe::store::write_spans(&pool, &[span])
        .await
        .unwrap();

    let rotated = platform::observe::parquet::rotate_spans(&state)
        .await
        .unwrap();
    assert!(rotated >= 1);

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM spans WHERE service = 'rotate-span-svc'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 0);
}

/// Rotate old metrics to parquet.
#[sqlx::test(migrations = "./migrations")]
async fn rotate_metrics_archives_old_data(pool: PgPool) {
    let state = test_state(pool.clone()).await;

    // Metric samples > 1h old
    let old_ts = Utc::now() - chrono::Duration::hours(2);
    let metric = platform::observe::store::MetricRecord {
        name: "rotate_metric_test".into(),
        labels: serde_json::json!({"host": "test"}),
        metric_type: "gauge".into(),
        unit: None,
        project_id: None,
        timestamp: old_ts,
        value: 42.0,
    };
    platform::observe::store::write_metrics(&pool, &[metric])
        .await
        .unwrap();

    let rotated = platform::observe::parquet::rotate_metrics(&state)
        .await
        .unwrap();
    assert!(rotated >= 1);
}
