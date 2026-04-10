// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Event-driven K8s watcher: streams pod/deployment state via reflectors,
//! flushes gauge metrics to `metric_samples` every 30s.
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

use super::store::{self, MetricRecord};
use crate::store::AppState;

/// Background task: stream K8s events into in-memory reflector stores,
/// flush state as gauge metrics to `metric_samples` every 30s.
#[tracing::instrument(skip_all, fields(namespace = %namespace))]
pub async fn run(state: AppState, namespace: String, cancel: tokio_util::sync::CancellationToken) {
    let pods_api: Api<Pod> = Api::namespaced(state.kube.clone(), &namespace);
    let deps_api: Api<Deployment> = Api::namespaced(state.kube.clone(), &namespace);

    // Set up reflector stores (thread-safe in-memory caches)
    let (pod_store, pod_writer) = reflector::store();
    let (dep_store, dep_writer) = reflector::store();

    // Stream K8s events into the reflector caches.
    let pod_stream =
        reflector::reflector(pod_writer, watcher(pods_api, watcher::Config::default()))
            .default_backoff()
            .applied_objects();
    let dep_stream =
        reflector::reflector(dep_writer, watcher(deps_api, watcher::Config::default()))
            .default_backoff()
            .applied_objects();

    // Spawn stream consumers in the background
    tokio::spawn(async move {
        tokio::pin!(pod_stream);
        while pod_stream.next().await.is_some() {}
    });
    tokio::spawn(async move {
        tokio::pin!(dep_stream);
        while dep_stream.next().await.is_some() {}
    });

    // Flush loop: read local in-memory stores, write to DB.
    let mut ticker = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = flush_stores(&state, &pod_store, &dep_store).await {
                    tracing::warn!(error = %e, "k8s metric flush failed");
                }
            }
            () = cancel.cancelled() => break,
        }
    }
}

/// Read the in-memory reflector stores and write metrics to Postgres.
/// No K8s API calls — purely local memory reads + DB writes.
async fn flush_stores(
    state: &AppState,
    pod_store: &reflector::Store<Pod>,
    dep_store: &reflector::Store<Deployment>,
) -> anyhow::Result<()> {
    let mut metrics = Vec::new();

    // Deployments → replicas, ready_replicas
    for dep in dep_store.state() {
        let name = dep.metadata.name.as_deref().unwrap_or("");
        let labels = serde_json::json!({"service": name});
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

    // Pods → restarts, OOM kills, ready status, resource requests/limits
    for pod in pod_store.state() {
        let service = pod_owner_name(&pod);
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let labels = serde_json::json!({"service": service, "pod": pod_name});

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
                    let svc_labels = serde_json::json!({"service": service});
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

    store::write_metrics(&state.pool, &metrics).await?;
    Ok(())
}

fn push_gauge(metrics: &mut Vec<MetricRecord>, name: &str, labels: &serde_json::Value, value: f64) {
    metrics.push(MetricRecord {
        name: name.into(),
        labels: labels.clone(),
        metric_type: "gauge".into(),
        unit: None,
        project_id: None, // platform infrastructure has no project_id
        timestamp: chrono::Utc::now(),
        value,
    });
}

/// Get the owner name for a pod (deployment name from ownerReferences).
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
/// `"500m"` → 500.0, `"2"` → 2000.0, `"0.5"` → 500.0
pub fn parse_cpu(quantity: Option<&Quantity>) -> f64 {
    let s = match quantity {
        Some(q) => &q.0,
        None => return 0.0,
    };
    if let Some(millis) = s.strip_suffix('m') {
        millis.parse::<f64>().unwrap_or(0.0)
    } else {
        // Whole cores or fractional: "2" → 2000, "0.5" → 500
        s.parse::<f64>().unwrap_or(0.0) * 1000.0
    }
}

/// Parse K8s memory quantity string to bytes.
/// `"512Mi"` → 536870912, `"1Gi"` → 1073741824, `"1000000"` → 1000000
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
        // SI: 10^3
        v.parse::<f64>().unwrap_or(0.0) * 1000.0
    } else if let Some(v) = s.strip_suffix('M') {
        // SI: 10^6
        v.parse::<f64>().unwrap_or(0.0) * 1_000_000.0
    } else if let Some(v) = s.strip_suffix('G') {
        // SI: 10^9
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
}
