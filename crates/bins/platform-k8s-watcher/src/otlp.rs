// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Minimal OTLP metrics-only HTTP client.
//!
//! Converts `MetricRecord`s to protobuf and POSTs to the ingest endpoint.
//! Never crashes on failure — logs warnings and moves on.

use std::time::Duration;

use chrono::{DateTime, Utc};
use prost::Message;

use platform_observe::proto;
use platform_observe::types::MetricRecord;

/// OTLP metrics HTTP client.
pub struct OtlpMetricsClient {
    client: reqwest::Client,
    endpoint: String,
    token: String,
}

impl OtlpMetricsClient {
    /// Create a new client with a 10s timeout.
    pub fn new(endpoint: String, token: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            client,
            endpoint,
            token,
        }
    }

    /// Send metrics to the ingest endpoint as OTLP protobuf.
    #[tracing::instrument(skip(self, metrics), fields(count = metrics.len()))]
    pub async fn send_metrics(&self, metrics: &[MetricRecord]) {
        if metrics.is_empty() {
            return;
        }

        let proto_metrics: Vec<proto::Metric> = metrics.iter().map(metric_to_proto).collect();

        let request = proto::ExportMetricsServiceRequest {
            resource_metrics: vec![proto::ResourceMetrics {
                resource: Some(proto::Resource {
                    attributes: vec![proto::KeyValue {
                        key: "service.name".into(),
                        value: Some(proto::AnyValue {
                            value: Some(proto::any_value::Value::StringValue(
                                "platform-k8s-watcher".into(),
                            )),
                        }),
                    }],
                }),
                scope_metrics: vec![proto::ScopeMetrics {
                    scope: Some(proto::InstrumentationScope {
                        name: "platform-k8s-watcher".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                    }),
                    metrics: proto_metrics,
                }],
            }],
        };

        let body = request.encode_to_vec();
        let url = format!("{}/v1/metrics", self.endpoint);

        match self
            .client
            .post(&url)
            .header("Content-Type", "application/x-protobuf")
            .header("Authorization", format!("Bearer {}", self.token))
            .body(body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                tracing::debug!(count = metrics.len(), "metrics flushed to ingest");
            }
            Ok(r) => {
                tracing::warn!(
                    status = r.status().as_u16(),
                    "OTLP metrics export got non-success response"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "OTLP metrics export failed");
            }
        }
    }
}

/// Convert a `MetricRecord` to a protobuf `Metric`.
fn metric_to_proto(metric: &MetricRecord) -> proto::Metric {
    let mut attrs = Vec::new();
    if let Some(map) = metric.labels.as_object() {
        for (k, v) in map {
            attrs.push(proto::KeyValue {
                key: k.clone(),
                value: Some(json_to_any_value(v)),
            });
        }
    }

    let time_ns = datetime_to_nanos(metric.timestamp);

    let data_point = proto::NumberDataPoint {
        attributes: attrs,
        time_unix_nano: time_ns,
        value: Some(proto::number_data_point::Value::AsDouble(metric.value)),
    };

    let data = match metric.metric_type.as_str() {
        "sum" => Some(proto::metric_data::Data::Sum(proto::Sum {
            data_points: vec![data_point],
            is_monotonic: true,
        })),
        _ => Some(proto::metric_data::Data::Gauge(proto::Gauge {
            data_points: vec![data_point],
        })),
    };

    proto::Metric {
        name: metric.name.clone(),
        description: String::new(),
        unit: metric.unit.clone().unwrap_or_default(),
        data,
    }
}

/// Convert a `serde_json::Value` to an OTLP `AnyValue`.
fn json_to_any_value(v: &serde_json::Value) -> proto::AnyValue {
    match v {
        serde_json::Value::String(s) => proto::AnyValue {
            value: Some(proto::any_value::Value::StringValue(s.clone())),
        },
        serde_json::Value::Bool(b) => proto::AnyValue {
            value: Some(proto::any_value::Value::BoolValue(*b)),
        },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                proto::AnyValue {
                    value: Some(proto::any_value::Value::IntValue(i)),
                }
            } else {
                proto::AnyValue {
                    value: Some(proto::any_value::Value::DoubleValue(
                        n.as_f64().unwrap_or(0.0),
                    )),
                }
            }
        }
        _ => proto::AnyValue {
            value: Some(proto::any_value::Value::StringValue(v.to_string())),
        },
    }
}

/// Convert a `DateTime<Utc>` to nanoseconds since epoch.
fn datetime_to_nanos(dt: DateTime<Utc>) -> u64 {
    u64::try_from(dt.timestamp_nanos_opt().unwrap_or(0)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_to_proto_gauge() {
        let metric = MetricRecord {
            name: "k8s.pod.restarts".into(),
            labels: serde_json::json!({"service": "api", "pod": "api-abc"}),
            metric_type: "gauge".into(),
            unit: None,
            project_id: None,
            timestamp: Utc::now(),
            value: 3.0,
        };

        let proto = metric_to_proto(&metric);
        assert_eq!(proto.name, "k8s.pod.restarts");
        assert!(matches!(
            proto.data,
            Some(proto::metric_data::Data::Gauge(_))
        ));
        // Check attributes were converted
        assert!(
            proto
                .data
                .as_ref()
                .and_then(|d| match d {
                    proto::metric_data::Data::Gauge(g) => g.data_points.first(),
                    _ => None,
                })
                .map_or(false, |dp| dp.attributes.len() == 2)
        );
    }

    #[test]
    fn metric_to_proto_sum() {
        let metric = MetricRecord {
            name: "http.requests".into(),
            labels: serde_json::json!({}),
            metric_type: "sum".into(),
            unit: Some("{request}".into()),
            project_id: None,
            timestamp: Utc::now(),
            value: 42.0,
        };

        let proto = metric_to_proto(&metric);
        assert!(matches!(proto.data, Some(proto::metric_data::Data::Sum(_))));
        assert_eq!(proto.unit, "{request}");
    }

    #[test]
    fn json_to_any_value_string() {
        let v = serde_json::json!("hello");
        let av = json_to_any_value(&v);
        assert!(matches!(
            av.value,
            Some(proto::any_value::Value::StringValue(ref s)) if s == "hello"
        ));
    }

    #[test]
    fn json_to_any_value_int() {
        let v = serde_json::json!(42);
        let av = json_to_any_value(&v);
        assert!(matches!(
            av.value,
            Some(proto::any_value::Value::IntValue(42))
        ));
    }

    #[test]
    fn json_to_any_value_bool() {
        let v = serde_json::json!(true);
        let av = json_to_any_value(&v);
        assert!(matches!(
            av.value,
            Some(proto::any_value::Value::BoolValue(true))
        ));
    }

    #[test]
    fn json_to_any_value_float() {
        let v = serde_json::json!(3.14);
        let av = json_to_any_value(&v);
        match av.value {
            Some(proto::any_value::Value::DoubleValue(d)) => {
                assert!((d - 3.14).abs() < 0.001);
            }
            _ => panic!("expected DoubleValue"),
        }
    }

    #[test]
    fn json_to_any_value_fallback() {
        let v = serde_json::json!([1, 2, 3]);
        let av = json_to_any_value(&v);
        assert!(matches!(
            av.value,
            Some(proto::any_value::Value::StringValue(_))
        ));
    }

    #[test]
    fn datetime_to_nanos_positive() {
        let dt = Utc::now();
        let ns = datetime_to_nanos(dt);
        assert!(ns > 0);
    }

    #[test]
    fn client_creation() {
        let client = OtlpMetricsClient::new("http://localhost:8081".into(), "test-token".into());
        assert_eq!(client.endpoint, "http://localhost:8081");
    }
}
