// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Internal record types for batch insertion.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Span record ready for batch insertion.
pub struct SpanRecord {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub service: String,
    pub kind: String,
    pub status: String,
    pub attributes: Option<JsonValue>,
    pub events: Option<JsonValue>,
    pub duration_ms: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub project_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

/// Log entry record ready for batch insertion.
pub struct LogEntryRecord {
    pub timestamp: DateTime<Utc>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub project_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub service: String,
    pub level: String,
    pub source: String,
    pub message: String,
    pub attributes: Option<JsonValue>,
}

/// Metric sample record ready for batch insertion.
pub struct MetricRecord {
    pub name: String,
    pub labels: JsonValue,
    pub metric_type: String,
    pub unit: Option<String>,
    pub project_id: Option<Uuid>,
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// Lightweight log message for live tail pub/sub.
#[derive(Debug, Serialize)]
pub struct LogTailMessage {
    pub timestamp: DateTime<Utc>,
    pub service: String,
    pub level: String,
    pub source: String,
    pub message: String,
    pub trace_id: Option<String>,
}
