// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! HMAC-SHA256 webhook payload signing.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Sign a payload with HMAC-SHA256, returning `"sha256={hex}"`.
///
/// Returns `None` if the secret is empty or HMAC initialization fails.
pub fn sign_payload(secret: &str, body: &[u8]) -> Option<String> {
    if secret.is_empty() {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(body);
    let signature = hex::encode(mac.finalize().into_bytes());
    Some(format!("sha256={signature}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_produces_sha256_prefix() {
        let sig = sign_payload("my-secret", b"hello").unwrap();
        assert!(sig.starts_with("sha256="));
        // hex hash is 64 chars
        assert_eq!(sig.len(), 7 + 64);
    }

    #[test]
    fn sign_is_deterministic() {
        let a = sign_payload("secret", b"payload").unwrap();
        let b = sign_payload("secret", b"payload").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_secrets_different_signatures() {
        let a = sign_payload("secret-1", b"payload").unwrap();
        let b = sign_payload("secret-2", b"payload").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn empty_secret_returns_none() {
        assert!(sign_payload("", b"payload").is_none());
    }

    #[test]
    fn known_test_vector() {
        // HMAC-SHA256("key", "The quick brown fox jumps over the lazy dog")
        // Expected: f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
        let sig = sign_payload("key", b"The quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(
            sig,
            "sha256=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn sign_empty_payload() {
        let sig = sign_payload("secret", b"").unwrap();
        assert!(sig.starts_with("sha256="));
        assert_eq!(sig.len(), 7 + 64);
    }

    #[test]
    fn different_payloads_different_signatures() {
        let a = sign_payload("secret", b"payload-1").unwrap();
        let b = sign_payload("secret", b"payload-2").unwrap();
        assert_ne!(a, b);
    }
}
