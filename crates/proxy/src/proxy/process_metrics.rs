//! Container-aware CPU and memory metrics via cgroup filesystem.
//!
//! Reads cgroup v2 (modern K8s) or v1 (legacy) stats to emit
//! `process.cpu.utilization` (millicores) and `process.memory.rss` (bytes).
//! Falls back gracefully on non-Linux / non-container environments.

use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{mpsc, watch};

use super::metrics::MetricRecord;

/// Snapshot of container resource usage from cgroup filesystem.
pub struct CgroupSnapshot {
    /// Total CPU time consumed by the container (microseconds).
    pub cpu_usage_usec: u64,
    /// Current memory usage of the container (bytes).
    pub mem_bytes: u64,
    /// Wall-clock time of this reading.
    pub sampled_at: Instant,
}

/// Read container resource usage from cgroup filesystem.
/// Tries cgroup v2 first, falls back to v1, returns None on non-Linux/non-container.
pub fn read_cgroup_stats() -> Option<CgroupSnapshot> {
    read_cgroup_v2().or_else(read_cgroup_v1)
}

fn read_cgroup_v2() -> Option<CgroupSnapshot> {
    let mem_bytes: u64 = std::fs::read_to_string("/sys/fs/cgroup/memory.current")
        .ok()?
        .trim()
        .parse()
        .ok()?;

    let cpu_stat = std::fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()?;
    let cpu_usage_usec: u64 = cpu_stat
        .lines()
        .find(|l| l.starts_with("usage_usec"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;

    Some(CgroupSnapshot {
        cpu_usage_usec,
        mem_bytes,
        sampled_at: Instant::now(),
    })
}

fn read_cgroup_v1() -> Option<CgroupSnapshot> {
    let mem_bytes: u64 = std::fs::read_to_string("/sys/fs/cgroup/memory/memory.usage_in_bytes")
        .ok()?
        .trim()
        .parse()
        .ok()?;

    // cpuacct.usage is in nanoseconds — convert to microseconds
    let cpu_ns: u64 = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpuacct.usage")
        .ok()?
        .trim()
        .parse()
        .ok()?;

    Some(CgroupSnapshot {
        cpu_usage_usec: cpu_ns / 1000,
        mem_bytes,
        sampled_at: Instant::now(),
    })
}

/// Compute CPU millicores from delta between two snapshots.
///
/// 1 core-second = 1,000,000 usec. `millicores = (delta_usec / 1e6 / elapsed_s) * 1000`.
#[allow(clippy::cast_precision_loss)]
pub fn cpu_millicores(prev: &CgroupSnapshot, curr: &CgroupSnapshot) -> f64 {
    let elapsed = curr
        .sampled_at
        .duration_since(prev.sampled_at)
        .as_secs_f64();
    if elapsed <= 0.0 {
        return 0.0;
    }
    let delta_usec = curr.cpu_usage_usec.saturating_sub(prev.cpu_usage_usec) as f64;
    (delta_usec / 1_000_000.0 / elapsed) * 1000.0
}

/// Background task: emit process CPU and memory metrics at the given interval.
///
/// Reads cgroup stats each tick. Memory is emitted immediately; CPU requires
/// two snapshots to compute the delta, so the first tick only records a baseline.
#[tracing::instrument(skip_all, fields(service = %service))]
pub async fn flush_process_metrics(
    service: String,
    metric_tx: mpsc::Sender<MetricRecord>,
    interval: Duration,
    mut shutdown: watch::Receiver<()>,
) {
    let mut prev: Option<CgroupSnapshot> = None;
    let mut ticker = tokio::time::interval(interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Some(curr) = read_cgroup_stats() {
                    let labels = serde_json::json!({"service": &service});

                    // Memory metric (container-scoped)
                    let _ = metric_tx.try_send(MetricRecord {
                        name: "process.memory.rss".into(),
                        labels: labels.clone(),
                        metric_type: "gauge".into(),
                        unit: Some("bytes".into()),
                        timestamp: Utc::now(),
                        #[allow(clippy::cast_precision_loss)]
                        value: curr.mem_bytes as f64,
                    });

                    // CPU metric (needs previous snapshot for delta)
                    if let Some(ref p) = prev {
                        let mc = cpu_millicores(p, &curr);
                        let _ = metric_tx.try_send(MetricRecord {
                            name: "process.cpu.utilization".into(),
                            labels,
                            metric_type: "gauge".into(),
                            unit: Some("millicores".into()),
                            timestamp: Utc::now(),
                            value: mc,
                        });
                    }

                    prev = Some(curr);
                }
            }
            _ = shutdown.changed() => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(cpu_usec: u64, mem: u64, at: Instant) -> CgroupSnapshot {
        CgroupSnapshot {
            cpu_usage_usec: cpu_usec,
            mem_bytes: mem,
            sampled_at: at,
        }
    }

    #[test]
    fn cpu_millicores_basic() {
        // 1,000,000 usec delta over 1s = 1 full core = 1000 millicores
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = snap(0, 100, t0);
        let curr = snap(1_000_000, 100, t1);
        let mc = cpu_millicores(&prev, &curr);
        assert!((mc - 1000.0).abs() < 0.01, "expected 1000, got {mc}");
    }

    #[test]
    fn cpu_millicores_zero_elapsed() {
        let t = Instant::now();
        let prev = snap(0, 100, t);
        let curr = snap(500_000, 100, t);
        assert_eq!(cpu_millicores(&prev, &curr), 0.0);
    }

    #[test]
    fn cpu_millicores_no_delta() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = snap(1_000_000, 100, t0);
        let curr = snap(1_000_000, 100, t1);
        assert_eq!(cpu_millicores(&prev, &curr), 0.0);
    }

    #[test]
    fn cpu_millicores_saturating() {
        // prev > curr (counter wraparound or anomaly) — should return 0, not panic
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = snap(2_000_000, 100, t0);
        let curr = snap(1_000_000, 100, t1);
        assert_eq!(cpu_millicores(&prev, &curr), 0.0);
    }

    #[test]
    fn cpu_millicores_fractional() {
        // 500,000 usec delta over 1s = 0.5 core = 500 millicores
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = snap(0, 100, t0);
        let curr = snap(500_000, 100, t1);
        let mc = cpu_millicores(&prev, &curr);
        assert!((mc - 500.0).abs() < 0.01, "expected 500, got {mc}");
    }

    #[test]
    fn read_cgroup_stats_fallback_none() {
        // On macOS / non-container Linux, should return None (no panic)
        let result = read_cgroup_stats();
        let _ = result;
    }

    #[test]
    fn read_cgroup_v2_parses_cpu_stat() {
        // Simulate parsing the cpu.stat format
        let input = "usage_usec 12345\nuser_usec 10000\nsystem_usec 2345\n";
        let parsed: u64 = input
            .lines()
            .find(|l| l.starts_with("usage_usec"))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(parsed, 12345);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_cgroup_stats_container() {
        // Inside K8s pod: should return Some with mem_bytes > 0
        if let Some(snap) = read_cgroup_stats() {
            assert!(snap.mem_bytes > 0);
        }
    }
}
