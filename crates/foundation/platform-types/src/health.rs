// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! In-memory heartbeat tracker for background tasks.
//!
//! Provides [`TaskRegistry`] which implements [`TaskHeartbeat`] for registering
//! background tasks and tracking their health via periodic heartbeats.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::traits::TaskHeartbeat;

// ---------------------------------------------------------------------------
// Health status enum (subset of src/health/types.rs)
// ---------------------------------------------------------------------------

/// Status of a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

// ---------------------------------------------------------------------------
// Snapshot type
// ---------------------------------------------------------------------------

/// Health snapshot for a single background task.
#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub name: String,
    pub status: TaskStatus,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub success_count: u64,
    pub failure_count: u64,
    pub last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal heartbeat entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct HeartbeatEntry {
    last_beat: Instant,
    last_beat_utc: DateTime<Utc>,
    success_count: u64,
    failure_count: u64,
    last_error: Option<String>,
    /// Expected interval in seconds. Task is "stale" if 3x this elapses.
    expected_interval_secs: u64,
}

// ---------------------------------------------------------------------------
// TaskRegistry
// ---------------------------------------------------------------------------

/// In-memory heartbeat tracker for background tasks.
///
/// Thread-safe (uses `RwLock`) and cheaply cloneable (uses `Arc`).
/// Each background task calls [`heartbeat`](TaskRegistry::heartbeat) on its
/// loop iteration; the health endpoint queries [`snapshot`](TaskRegistry::snapshot)
/// to build a status page.
#[derive(Debug, Clone)]
pub struct TaskRegistry {
    tasks: Arc<RwLock<HashMap<String, HeartbeatEntry>>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a successful heartbeat for a named task.
    pub fn heartbeat(&self, name: &str) {
        if let Ok(mut map) = self.tasks.write() {
            let entry = map
                .entry(name.to_owned())
                .or_insert_with(|| HeartbeatEntry {
                    last_beat: Instant::now(),
                    last_beat_utc: Utc::now(),
                    success_count: 0,
                    failure_count: 0,
                    last_error: None,
                    expected_interval_secs: 30,
                });
            entry.last_beat = Instant::now();
            entry.last_beat_utc = Utc::now();
            entry.success_count += 1;
        }
    }

    /// Record an error for a named task.
    pub fn report_error(&self, name: &str, err: &str) {
        if let Ok(mut map) = self.tasks.write() {
            let entry = map
                .entry(name.to_owned())
                .or_insert_with(|| HeartbeatEntry {
                    last_beat: Instant::now(),
                    last_beat_utc: Utc::now(),
                    success_count: 0,
                    failure_count: 0,
                    last_error: None,
                    expected_interval_secs: 30,
                });
            entry.last_beat = Instant::now();
            entry.last_beat_utc = Utc::now();
            entry.failure_count += 1;
            entry.last_error = Some(err.to_owned());
        }
    }

    /// Register a task with its expected interval (in seconds).
    pub fn register(&self, name: &str, expected_interval_secs: u64) {
        if let Ok(mut map) = self.tasks.write() {
            map.entry(name.to_owned())
                .or_insert_with(|| HeartbeatEntry {
                    last_beat: Instant::now(),
                    last_beat_utc: Utc::now(),
                    success_count: 0,
                    failure_count: 0,
                    last_error: None,
                    expected_interval_secs,
                });
        }
    }

    /// Check if a named task is healthy (not stale).
    /// Returns `true` if the task is not registered (startup race).
    pub fn is_healthy(&self, name: &str) -> bool {
        let tasks = self
            .tasks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match tasks.get(name) {
            Some(hb) => {
                let elapsed = Instant::now().duration_since(hb.last_beat);
                let stale_threshold = std::time::Duration::from_secs(hb.expected_interval_secs * 3);
                elapsed <= stale_threshold
            }
            None => true, // Not registered yet — assume healthy
        }
    }

    /// Build a snapshot of all tasks' health.
    pub fn snapshot(&self) -> Vec<TaskSnapshot> {
        let Ok(map) = self.tasks.read() else {
            return Vec::new();
        };
        let now = Instant::now();
        let mut tasks: Vec<TaskSnapshot> = map
            .iter()
            .map(|(name, hb)| {
                let stale_threshold = std::time::Duration::from_secs(hb.expected_interval_secs * 3);
                let elapsed = now.duration_since(hb.last_beat);
                let status = if elapsed > stale_threshold {
                    TaskStatus::Unhealthy
                } else if hb.last_error.is_some() {
                    TaskStatus::Degraded
                } else {
                    TaskStatus::Healthy
                };
                TaskSnapshot {
                    name: name.clone(),
                    status,
                    last_heartbeat: Some(hb.last_beat_utc),
                    success_count: hb.success_count,
                    failure_count: hb.failure_count,
                    last_error: hb.last_error.clone(),
                }
            })
            .collect();
        // Sort unhealthy first, then by name
        tasks.sort_by(|a, b| {
            let order = |s: &TaskStatus| match s {
                TaskStatus::Unhealthy => 0,
                TaskStatus::Degraded => 1,
                TaskStatus::Healthy => 2,
            };
            order(&a.status)
                .cmp(&order(&b.status))
                .then(a.name.cmp(&b.name))
        });
        tasks
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskHeartbeat for TaskRegistry {
    fn register(&self, name: &str, expected_interval_secs: u64) {
        TaskRegistry::register(self, name, expected_interval_secs);
    }

    fn heartbeat(&self, name: &str) {
        TaskRegistry::heartbeat(self, name);
    }

    fn report_error(&self, name: &str, message: &str) {
        TaskRegistry::report_error(self, name, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_heartbeat_increments() {
        let registry = TaskRegistry::new();
        registry.heartbeat("test-task");
        registry.heartbeat("test-task");
        let snap = registry.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "test-task");
        assert_eq!(snap[0].success_count, 2);
        assert_eq!(snap[0].failure_count, 0);
        assert_eq!(snap[0].status, TaskStatus::Healthy);
    }

    #[test]
    fn registry_report_error() {
        let registry = TaskRegistry::new();
        registry.heartbeat("task-a");
        registry.report_error("task-a", "connection refused");
        let snap = registry.snapshot();
        assert_eq!(snap[0].success_count, 1);
        assert_eq!(snap[0].failure_count, 1);
        assert_eq!(snap[0].last_error.as_deref(), Some("connection refused"));
        assert_eq!(snap[0].status, TaskStatus::Degraded);
    }

    #[test]
    fn registry_register_sets_interval() {
        let registry = TaskRegistry::new();
        registry.register("slow-task", 3600);
        let snap = registry.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "slow-task");
        assert_eq!(snap[0].status, TaskStatus::Healthy);
    }

    #[test]
    fn registry_is_healthy_unregistered() {
        let registry = TaskRegistry::new();
        assert!(registry.is_healthy("nonexistent"));
    }

    #[test]
    fn registry_is_healthy_fresh() {
        let registry = TaskRegistry::new();
        registry.register("test", 30);
        registry.heartbeat("test");
        assert!(registry.is_healthy("test"));
    }

    #[test]
    fn registry_default_is_empty() {
        let registry = TaskRegistry::default();
        assert!(registry.snapshot().is_empty());
    }

    #[test]
    fn registry_trait_impl() {
        // Verify TaskHeartbeat trait methods work
        let registry = TaskRegistry::new();
        TaskHeartbeat::register(&registry, "task", 60);
        TaskHeartbeat::heartbeat(&registry, "task");
        TaskHeartbeat::report_error(&registry, "task", "oops");
        let snap = registry.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].success_count, 1);
        assert_eq!(snap[0].failure_count, 1);
    }

    #[test]
    fn snapshot_sorts_unhealthy_first() {
        let registry = TaskRegistry::new();
        registry.heartbeat("healthy-task");
        registry.report_error("degraded-task", "oops");
        let snap = registry.snapshot();
        assert_eq!(snap[0].name, "degraded-task");
        assert_eq!(snap[1].name, "healthy-task");
    }
}
