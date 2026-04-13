// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Mesh-specific configuration structs.
//!
//! Owned structs — callers construct these from their own `Config` fields.
//! No dependency on the main binary's `Config` type.

/// Configuration for the mesh certificate authority.
#[derive(Debug, Clone)]
pub struct MeshConfig {
    /// Hex-encoded 32-byte master key for encrypting the root CA private key.
    pub master_key_hex: Option<String>,
    /// Root CA certificate TTL in days (e.g. 3650 for ~10 years).
    pub root_ttl_days: u32,
    /// Leaf certificate TTL in seconds (e.g. 86400 for 24 hours).
    pub cert_ttl_secs: u64,
}

/// Configuration for the ACME HTTP-01 certificate manager.
#[derive(Debug, Clone)]
pub struct AcmeConfig {
    /// ACME directory URL (e.g. Let's Encrypt production or staging).
    pub directory_url: String,
    /// Contact email for ACME account registration.
    pub contact_email: Option<String>,
    /// Gateway namespace where TLS secrets and ACME configmap live.
    pub gateway_namespace: String,
    /// Gateway name to filter `HTTPRoute` parentRefs.
    pub gateway_name: String,
    /// Check interval in seconds (default: 3600 = 1 hour).
    pub check_interval_secs: u64,
}
