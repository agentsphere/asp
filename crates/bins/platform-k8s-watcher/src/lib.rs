// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Standalone K8s watcher library.
//!
//! Watches pods and deployments across the cluster, collects gauge metrics
//! (restarts, OOM kills, ready status, resource requests/limits), and sends
//! them to `platform-ingest` via OTLP HTTP protobuf.

pub mod config;
pub mod otlp;
pub mod watcher;
