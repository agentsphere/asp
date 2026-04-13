// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Observe domain: OTLP proto types, record types, store writes,
//! correlation, ingest pipeline, and alert state machine.

pub mod alert;
pub mod correlation;
pub mod error;
pub mod ingest;
pub mod proto;
pub mod store;
pub mod types;
