// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Event-driven K8s watcher: streams pod/deployment state via reflectors,
//! flushes gauge metrics to the ingest endpoint via OTLP.
//!
//! Zero K8s API calls at flush time — reads from in-memory reflector stores.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::Api;
use kube::runtime::{WatchStreamExt, reflector, watcher};
use tokio_util::sync::CancellationToken;

use platform_observe::types::MetricRecord;

use crate::otlp::OtlpMetricsClient;

/// Run the K8s watcher. Streams events into reflector stores and flushes
/// metrics to the OTLP ingest endpoint at the configured interval.
#[tracing::instrument(skip_all)]
pub async fn run(
    kube_client: kube::Client,
    otlp_client: Arc<OtlpMetricsClient>,
    namespaces: Vec<String>,
    flush_interval: Duration,
    cancel: CancellationToken,
) {
    let mut pod_stores = Vec::new();
    let mut dep_stores = Vec::new();

    if namespaces.is_empty() {
        // Cluster-wide watching
        tracing::info!("watching all namespaces");
        let pods_api: Api<Pod> = Api::all(kube_client.clone());
        let deps_api: Api<Deployment> = Api::all(kube_client.clone());

        let (pod_store, pod_writer) = reflector::store();
        let (dep_store, dep_writer) = reflector::store();

        let pod_stream =
            reflector::reflector(pod_writer, watcher(pods_api, watcher::Config::default()))
                .default_backoff()
                .applied_objects();
        let dep_stream =
            reflector::reflector(dep_writer, watcher(deps_api, watcher::Config::default()))
                .default_backoff()
                .applied_objects();

        tokio::spawn(async move {
            tokio::pin!(pod_stream);
            while pod_stream.next().await.is_some() {}
        });
        tokio::spawn(async move {
            tokio::pin!(dep_stream);
            while dep_stream.next().await.is_some() {}
        });

        pod_stores.push(pod_store);
        dep_stores.push(dep_store);
    } else {
        // Per-namespace watching
        for ns in &namespaces {
            tracing::info!(namespace = %ns, "watching namespace");
            let pods_api: Api<Pod> = Api::namespaced(kube_client.clone(), ns);
            let deps_api: Api<Deployment> = Api::namespaced(kube_client.clone(), ns);

            let (pod_store, pod_writer) = reflector::store();
            let (dep_store, dep_writer) = reflector::store();

            let pod_stream =
                reflector::reflector(pod_writer, watcher(pods_api, watcher::Config::default()))
                    .default_backoff()
                    .applied_objects();
            let dep_stream =
                reflector::reflector(dep_writer, watcher(deps_api, watcher::Config::default()))
                    .default_backoff()
                    .applied_objects();

            tokio::spawn(async move {
                tokio::pin!(pod_stream);
                while pod_stream.next().await.is_some() {}
            });
            tokio::spawn(async move {
                tokio::pin!(dep_stream);
                while dep_stream.next().await.is_some() {}
            });

            pod_stores.push(pod_store);
            dep_stores.push(dep_store);
        }
    }

    // Flush loop: read local in-memory stores, send to ingest.
    let mut ticker = tokio::time::interval(flush_interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                flush_stores(&otlp_client, &pod_stores, &dep_stores).await;
            }
            () = cancel.cancelled() => break,
        }
    }

    tracing::info!("k8s watcher shutting down");
}

/// Read the in-memory reflector stores and send metrics via OTLP.
async fn flush_stores(
    otlp_client: &OtlpMetricsClient,
    pod_stores: &[reflector::Store<Pod>],
    dep_stores: &[reflector::Store<Deployment>],
) {
    let mut metrics = Vec::new();

    for dep_store in dep_stores {
        for dep in dep_store.state() {
            let name = dep.metadata.name.as_deref().unwrap_or("");
            let ns = dep.metadata.namespace.as_deref().unwrap_or("");
            let labels = serde_json::json!({"service": name, "namespace": ns});
            let status = dep.status.as_ref();
            push_gauge(
                &mut metrics,
                "k8s.deployment.replicas",
                &labels,
                f64::from(dep.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0)),
            );
            push_gauge(
                &mut metrics,
                "k8s.deployment.ready_replicas",
                &labels,
                f64::from(status.and_then(|s| s.ready_replicas).unwrap_or(0)),
            );
        }
    }

    for pod_store in pod_stores {
        for pod in pod_store.state() {
            let service = pod_owner_name(&pod);
            let pod_name = pod.metadata.name.as_deref().unwrap_or("");
            let ns = pod.metadata.namespace.as_deref().unwrap_or("");
            let labels = serde_json::json!({"service": service, "pod": pod_name, "namespace": ns});

            let mut restarts: i32 = 0;
            let mut ooms: i32 = 0;
            if let Some(status) = &pod.status {
                for cs in status.container_statuses.iter().flatten() {
                    restarts += cs.restart_count;
                    if let Some(last) = &cs.last_state
                        && let Some(term) = &last.terminated
                        && term.reason.as_deref() == Some("OOMKilled")
                    {
                        ooms += 1;
                    }
                }
                let is_ready = status
                    .conditions
                    .iter()
                    .flatten()
                    .any(|c| c.type_ == "Ready" && c.status == "True");
                push_gauge(
                    &mut metrics,
                    "k8s.pod.ready",
                    &labels,
                    if is_ready { 1.0 } else { 0.0 },
                );
            }
            push_gauge(
                &mut metrics,
                "k8s.pod.restarts",
                &labels,
                f64::from(restarts),
            );
            push_gauge(&mut metrics, "k8s.pod.oom_kills", &labels, f64::from(ooms));

            // Resource requests/limits from pod spec
            if let Some(spec) = &pod.spec {
                for container in &spec.containers {
                    if let Some(res) = &container.resources {
                        let svc_labels = serde_json::json!({"service": service, "namespace": ns});
                        push_gauge(
                            &mut metrics,
                            "k8s.container.cpu.request",
                            &svc_labels,
                            parse_cpu(res.requests.as_ref().and_then(|r| r.get("cpu"))),
                        );
                        push_gauge(
                            &mut metrics,
                            "k8s.container.cpu.limit",
                            &svc_labels,
                            parse_cpu(res.limits.as_ref().and_then(|r| r.get("cpu"))),
                        );
                        push_gauge(
                            &mut metrics,
                            "k8s.container.memory.request",
                            &svc_labels,
                            parse_mem(res.requests.as_ref().and_then(|r| r.get("memory"))),
                        );
                        push_gauge(
                            &mut metrics,
                            "k8s.container.memory.limit",
                            &svc_labels,
                            parse_mem(res.limits.as_ref().and_then(|r| r.get("memory"))),
                        );
                    }
                }
            }
        }
    }

    if !metrics.is_empty() {
        tracing::debug!(count = metrics.len(), "flushing k8s metrics");
        otlp_client.send_metrics(&metrics).await;
    }
}

fn push_gauge(metrics: &mut Vec<MetricRecord>, name: &str, labels: &serde_json::Value, value: f64) {
    metrics.push(MetricRecord {
        name: name.into(),
        labels: labels.clone(),
        metric_type: "gauge".into(),
        unit: None,
        project_id: None,
        timestamp: chrono::Utc::now(),
        value,
    });
}

/// Get the owner name for a pod (deployment name from `ownerReferences`).
fn pod_owner_name(pod: &Arc<Pod>) -> String {
    pod.metadata
        .owner_references
        .as_ref()
        .and_then(|refs| refs.first())
        .map_or_else(
            || {
                pod.metadata
                    .name
                    .as_deref()
                    .unwrap_or("unknown")
                    .to_string()
            },
            |r| {
                // ReplicaSet names are "<deployment>-<hash>"; strip the hash suffix
                if r.kind == "ReplicaSet" {
                    r.name
                        .rsplit_once('-')
                        .map_or(r.name.as_str(), |(prefix, _)| prefix)
                        .to_string()
                } else {
                    r.name.clone()
                }
            },
        )
}

/// Parse K8s CPU quantity string to millicores.
/// `"500m"` -> 500.0, `"2"` -> 2000.0, `"0.5"` -> 500.0
pub fn parse_cpu(quantity: Option<&Quantity>) -> f64 {
    let s = match quantity {
        Some(q) => &q.0,
        None => return 0.0,
    };
    if let Some(millis) = s.strip_suffix('m') {
        millis.parse::<f64>().unwrap_or(0.0)
    } else {
        // Whole cores or fractional: "2" -> 2000, "0.5" -> 500
        s.parse::<f64>().unwrap_or(0.0) * 1000.0
    }
}

/// Parse K8s memory quantity string to bytes.
/// `"512Mi"` -> 536870912, `"1Gi"` -> 1073741824, `"1000000"` -> 1000000
pub fn parse_mem(quantity: Option<&Quantity>) -> f64 {
    let s = match quantity {
        Some(q) => &q.0,
        None => return 0.0,
    };
    if let Some(v) = s.strip_suffix("Ki") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0
    } else if let Some(v) = s.strip_suffix("Mi") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix("Gi") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix("Ti") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0 * 1024.0 * 1024.0
    } else if let Some(v) = s.strip_suffix('K') {
        v.parse::<f64>().unwrap_or(0.0) * 1000.0
    } else if let Some(v) = s.strip_suffix('M') {
        v.parse::<f64>().unwrap_or(0.0) * 1_000_000.0
    } else if let Some(v) = s.strip_suffix('G') {
        v.parse::<f64>().unwrap_or(0.0) * 1_000_000_000.0
    } else {
        // Raw bytes
        s.parse::<f64>().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Quantity {
        Quantity(s.into())
    }

    #[test]
    fn parse_cpu_millicores() {
        assert!((parse_cpu(Some(&q("500m"))) - 500.0).abs() < 0.01);
    }

    #[test]
    fn parse_cpu_whole_cores() {
        assert!((parse_cpu(Some(&q("2"))) - 2000.0).abs() < 0.01);
    }

    #[test]
    fn parse_cpu_fractional() {
        assert!((parse_cpu(Some(&q("0.5"))) - 500.0).abs() < 0.01);
    }

    #[test]
    fn parse_cpu_none() {
        assert_eq!(parse_cpu(None), 0.0);
    }

    #[test]
    fn parse_mem_mebibytes() {
        assert!((parse_mem(Some(&q("512Mi"))) - 536_870_912.0).abs() < 1.0);
    }

    #[test]
    fn parse_mem_gibibytes() {
        assert!((parse_mem(Some(&q("1Gi"))) - 1_073_741_824.0).abs() < 1.0);
    }

    #[test]
    fn parse_mem_kibibytes() {
        assert!((parse_mem(Some(&q("256Ki"))) - 262_144.0).abs() < 1.0);
    }

    #[test]
    fn parse_mem_si_mega() {
        assert!((parse_mem(Some(&q("500M"))) - 500_000_000.0).abs() < 1.0);
    }

    #[test]
    fn parse_mem_raw_bytes() {
        assert!((parse_mem(Some(&q("1048576"))) - 1_048_576.0).abs() < 1.0);
    }

    #[test]
    fn parse_mem_none() {
        assert_eq!(parse_mem(None), 0.0);
    }

    // -----------------------------------------------------------------------
    // push_gauge
    // -----------------------------------------------------------------------

    #[test]
    fn push_gauge_creates_metric_record() {
        let mut metrics = Vec::new();
        let labels = serde_json::json!({"service": "web", "namespace": "default"});
        push_gauge(&mut metrics, "k8s.pod.restarts", &labels, 3.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "k8s.pod.restarts");
        assert_eq!(metrics[0].metric_type, "gauge");
        assert_eq!(metrics[0].value, 3.0);
        assert_eq!(metrics[0].labels["service"], "web");
        assert_eq!(metrics[0].labels["namespace"], "default");
        assert!(metrics[0].unit.is_none());
        assert!(metrics[0].project_id.is_none());
    }

    #[test]
    fn push_gauge_appends_to_existing() {
        let mut metrics = Vec::new();
        let labels = serde_json::json!({"service": "api"});
        push_gauge(&mut metrics, "m1", &labels, 1.0);
        push_gauge(&mut metrics, "m2", &labels, 2.0);
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "m1");
        assert_eq!(metrics[1].name, "m2");
    }

    // -----------------------------------------------------------------------
    // pod_owner_name
    // -----------------------------------------------------------------------

    fn make_pod(name: Option<&str>, owner_refs: Option<Vec<(&str, &str)>>) -> Arc<Pod> {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
        Arc::new(Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: name.map(String::from),
                owner_references: owner_refs.map(|refs| {
                    refs.into_iter()
                        .map(|(kind, n)| OwnerReference {
                            kind: kind.to_string(),
                            name: n.to_string(),
                            ..Default::default()
                        })
                        .collect()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    #[test]
    fn pod_owner_name_no_owner_uses_pod_name() {
        let pod = make_pod(Some("my-pod-abc"), None);
        assert_eq!(pod_owner_name(&pod), "my-pod-abc");
    }

    #[test]
    fn pod_owner_name_no_owner_no_name() {
        let pod = make_pod(None, None);
        assert_eq!(pod_owner_name(&pod), "unknown");
    }

    #[test]
    fn pod_owner_name_replicaset_strips_hash() {
        let pod = make_pod(
            Some("web-abc123-xyz"),
            Some(vec![("ReplicaSet", "web-abc123")]),
        );
        // "web-abc123" → rsplit_once('-') → ("web", "abc123") → "web"
        assert_eq!(pod_owner_name(&pod), "web");
    }

    #[test]
    fn pod_owner_name_replicaset_no_dash() {
        let pod = make_pod(Some("pod"), Some(vec![("ReplicaSet", "nodash")]));
        // No dash → rsplit_once returns None → use full name
        assert_eq!(pod_owner_name(&pod), "nodash");
    }

    #[test]
    fn pod_owner_name_replicaset_multi_dash() {
        let pod = make_pod(
            Some("pod"),
            Some(vec![("ReplicaSet", "my-cool-app-7f9c4b")]),
        );
        // rsplit_once('-') → ("my-cool-app", "7f9c4b") → "my-cool-app"
        assert_eq!(pod_owner_name(&pod), "my-cool-app");
    }

    #[test]
    fn pod_owner_name_non_replicaset_uses_full_name() {
        let pod = make_pod(Some("pod"), Some(vec![("StatefulSet", "redis-0")]));
        assert_eq!(pod_owner_name(&pod), "redis-0");
    }

    #[test]
    fn pod_owner_name_job_uses_full_name() {
        let pod = make_pod(Some("pod"), Some(vec![("Job", "batch-job-12345")]));
        assert_eq!(pod_owner_name(&pod), "batch-job-12345");
    }
}
