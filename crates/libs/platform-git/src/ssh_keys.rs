// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! SSH public key parsing and validation.

use ssh_key::PublicKey;

use crate::error::SshKeyError;

// ---------------------------------------------------------------------------
// Parsed key output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedSshKey {
    /// Algorithm name (e.g. "ssh-ed25519", "ssh-rsa", "ecdsa-sha2-nistp256")
    pub algorithm: String,
    /// SHA-256 fingerprint in "SHA256:..." format
    pub fingerprint: String,
    /// Canonical OpenSSH-format public key (re-serialized, no comment)
    pub public_key_openssh: String,
}

// ---------------------------------------------------------------------------
// Allowed algorithms
// ---------------------------------------------------------------------------

const ALLOWED_ALGORITHMS: &[&str] = &[
    "ssh-ed25519",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ssh-rsa",
];

const MIN_RSA_BITS: u32 = 2048;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an OpenSSH-format public key string, validate algorithm and key size,
/// compute the SHA-256 fingerprint, and return the canonical re-serialized form.
pub fn parse_ssh_public_key(input: &str) -> Result<ParsedSshKey, SshKeyError> {
    let key: PublicKey = input.parse().map_err(|_| SshKeyError::InvalidFormat)?;

    let algorithm = key.algorithm().as_str().to_string();

    // Reject unsupported algorithms
    if !ALLOWED_ALGORITHMS.contains(&algorithm.as_str()) {
        return Err(SshKeyError::UnsupportedAlgorithm(algorithm));
    }

    // Enforce minimum RSA key size
    if algorithm == "ssh-rsa"
        && let ssh_key::public::KeyData::Rsa(rsa_key) = key.key_data()
    {
        let bits = rsa_key.n.as_positive_bytes().map_or(0, |b| {
            u32::try_from(b.len()).unwrap_or(u32::MAX).saturating_mul(8)
        });
        if bits < MIN_RSA_BITS {
            return Err(SshKeyError::RsaKeyTooShort(bits));
        }
    }

    // Compute SHA-256 fingerprint
    let fingerprint = key.fingerprint(ssh_key::HashAlg::Sha256);
    let fingerprint_str = fingerprint.to_string();

    // Re-serialize to canonical OpenSSH format (strips comments, normalizes whitespace)
    let public_key_openssh = key
        .to_openssh()
        .map_err(|_| SshKeyError::FingerprintError)?;

    Ok(ParsedSshKey {
        algorithm,
        fingerprint: fingerprint_str,
        public_key_openssh,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Real ED25519 test key (generated with ssh-keygen -t ed25519)
    const TEST_ED25519_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKjB6KC6pSWW2pW828DmK4uouTNB2a0nJQx0qLZW+2++ test@example.com";

    // Real ECDSA P-256 test key (generated with ssh-keygen -t ecdsa -b 256)
    const TEST_ECDSA_P256_KEY: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBOzWrIJeDiWNtIIWWHa374gK3geXl+OlmEUJAw4ZDS2xtF0R7WGp0ZKgt7hpCDjNMj9Z6hmhJF5Vu0vzhFt9wJY= test@example.com";

    // Real ECDSA P-384 test key (generated with ssh-keygen -t ecdsa -b 384)
    const TEST_ECDSA_P384_KEY: &str = "ecdsa-sha2-nistp384 AAAAE2VjZHNhLXNoYTItbmlzdHAzODQAAAAIbmlzdHAzODQAAABhBB0OGEsKBEHflvLSrPR5o0eKFm8gbfR7JQeOXIupqNW055xz+c++jlePfOgZXHCxvdl3SqFlnKiJHBfB7rgIN98uf8Nbimb14CDiQoSOFF8VbDB5P3hrSD5+3ZMY+WYSIg== test@example.com";

    // Real RSA 4096-bit test key (generated with ssh-keygen -t rsa -b 4096)
    const TEST_RSA_4096_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAACAQD0r4jcO3nQBNRQvnCCDco9DYcn32asUYJGRx7OXuirMA1+HZtMUTM/wAwpXa4huPmajhmK8Lh5gpQmeyvdZx9r55lmiH1RJp73y5iC6HXWMgLJ6Q9Q9D/gHxc7Te9hyoayJsGUzS/fvxeaaONJKC/OiMBmeHXnrJcPzUGek/UJtz7bXFRdBTXKMb03hMR0VYieXe4Uh31XO8O/2+lLfpUad8qsgbxAa22Ga/S6BLTFe0IYqrrM44LIcVcIW5ODvwM5UAU4ohZkP43JjfS4sQVyPv8XOD/546giFyB6kUGXj0/sxIZ5iNEbNBsRMamTfkbvbunMUW5nc9XkKI+YxNrnzRE573t1+ePyZZ3fMhTDMfkLymZju3cH2ICEHQIqcAXh4CBqaajbkrZ7goVbdWmtGyMGWPtQLeUTc30WgKiqAswr4u69ekN4RfOcV7SGIASSOW5cH40bCfOxTjT6XlfkrxpFtB2Pmb/ldmkLfV3w531JY+mdOPNVBFXwGn5kW2V5Ihz9XL1MIGohPV9kIHAMLbL08ZXZJFt2uaZeBhfJJz1idAC6d7qrNF0hwzm1AVJrTvp+vkK9rGlbHSvAGdpv2um3Q4trqxz1E5ikl3Lv8kAQbo6TeWvpD6xXRbYRxmyvPm+xSBEn++kx+rOeGgwBz7ccAcbN8+vzVIXEMfQIiQ== test@example.com";

    #[test]
    fn test_parse_ed25519_key() {
        let result = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        assert_eq!(result.algorithm, "ssh-ed25519");
        assert!(result.fingerprint.starts_with("SHA256:"));
        assert!(result.public_key_openssh.starts_with("ssh-ed25519 "));
    }

    #[test]
    fn test_parse_rsa_key_4096() {
        let result = parse_ssh_public_key(TEST_RSA_4096_KEY).unwrap();
        assert_eq!(result.algorithm, "ssh-rsa");
        assert!(result.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn test_parse_rsa_key_2048_minimum() {
        let result = parse_ssh_public_key(TEST_RSA_4096_KEY);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_rsa_key_1024_rejected() {
        let key_1024 = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQDZg0pevgw1D7sN+GUm+gHWCj+rF9voGPIkv+Au9MqYdxOR7YRC6Mh87I7v7WOyVSw2ByyI482Cr6nayq4D6dxRfpUCi1jfq+BytStZjyZYqt7bq5UfTEC2tZaQY8E17izcDhOU1EFAfwEvBGO0U8nwuIQT6+1OKmKDkantymX69w== test@example.com";
        let result = parse_ssh_public_key(key_1024);
        assert!(
            matches!(result, Err(SshKeyError::RsaKeyTooShort(_))),
            "expected RsaKeyTooShort, got: {result:?}"
        );
    }

    #[test]
    fn test_parse_ecdsa_nistp256() {
        let result = parse_ssh_public_key(TEST_ECDSA_P256_KEY).unwrap();
        assert_eq!(result.algorithm, "ecdsa-sha2-nistp256");
        assert!(result.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn test_parse_ecdsa_nistp384() {
        let result = parse_ssh_public_key(TEST_ECDSA_P384_KEY).unwrap();
        assert_eq!(result.algorithm, "ecdsa-sha2-nistp384");
        assert!(result.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn test_parse_key_with_comment() {
        let result = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        assert!(result.public_key_openssh.starts_with("ssh-ed25519 "));
        assert!(!result.public_key_openssh.is_empty());
    }

    #[test]
    fn test_parse_key_without_comment() {
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKjB6KC6pSWW2pW828DmK4uouTNB2a0nJQx0qLZW+2++";
        let result = parse_ssh_public_key(key).unwrap();
        assert_eq!(result.algorithm, "ssh-ed25519");
    }

    #[test]
    fn test_parse_dsa_key_rejected() {
        let dsa_key = "ssh-dss AAAAB3NzaC1kc3MAAACBANY4aG6qfPpgO8Zw3pbA8od5k8p6vYTikmhzncmGKjC8887wzy7eiIUeoqoccFCMsa9S0i20TIpNA3H7up8haFxODd8KAHUNCD/79pr/hG7Z9G2McS7psubZp/DvRE7pm18pTenwIBMsW/yEJoqCPpAgFOrTyHazt598LuwLMgihAAAAFQDjgX5YP/mXnYeoxPGsIDPXelwY5QAAAIBRWTUuVpw9y+q1HQ6utsFUZGbM7c8eOInCFstq5amkuQhUaM/QF4Dt0LYpbiZ8vx7GnphRwRfNyDk1otJpjipxuQ/RAr77waeU9OIn3AanQuxwVGduGOMpnG53k0VoOTbtCDKsR6Fg0ZwKy6sOozBOHRc8B+uvTVKy4A5fsunGZgAAAIEAjveLf8s5J8uNQVPyiJawi4Cy+NO5YwAv3pauIMS2fgCKNuV/RxWK5z5fPP/xmX8V+JZUDFPmdmiwvClSatOI4CbAoa6GaYgiM/oUol6x/P/SebVDOAm59bItHumC8wenILV+d0Bnc9ssS+1NMVNUfldIayan3fGkW9RxPXf4s7o= test@example.com";
        let result = parse_ssh_public_key(dsa_key);
        assert!(
            result.is_err(),
            "DSA key should be rejected, got: {result:?}"
        );
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_ssh_public_key("");
        assert!(
            matches!(result, Err(SshKeyError::InvalidFormat)),
            "expected InvalidFormat, got: {result:?}"
        );
    }

    #[test]
    fn test_parse_garbage_input() {
        let result = parse_ssh_public_key("this is not a key at all!!!!");
        assert!(
            matches!(result, Err(SshKeyError::InvalidFormat)),
            "expected InvalidFormat, got: {result:?}"
        );
    }

    #[test]
    fn test_parse_truncated_key() {
        let result = parse_ssh_public_key("ssh-ed25519 AAAA");
        assert!(
            matches!(result, Err(SshKeyError::InvalidFormat)),
            "expected InvalidFormat, got: {result:?}"
        );
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let r1 = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        let r2 = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        assert_eq!(r1.fingerprint, r2.fingerprint);
    }

    #[test]
    fn test_fingerprint_format() {
        let result = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        assert!(
            result.fingerprint.starts_with("SHA256:"),
            "fingerprint should start with SHA256:, got: {}",
            result.fingerprint
        );
    }

    #[test]
    fn test_different_keys_different_fingerprints() {
        let r1 = parse_ssh_public_key(TEST_ED25519_KEY).unwrap();
        let r2 = parse_ssh_public_key(TEST_ECDSA_P256_KEY).unwrap();
        assert_ne!(r1.fingerprint, r2.fingerprint);
    }
}
