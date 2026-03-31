// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use chrono::{DateTime, Utc};
use pgp::composed::{Deserializable, SignedPublicKey};
use pgp::types::PublicKeyTrait;

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum GpgKeyError {
    #[error("invalid PGP public key armor")]
    InvalidArmor,
    #[error("failed to extract key metadata")]
    MetadataError,
}

impl From<GpgKeyError> for ApiError {
    fn from(err: GpgKeyError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Parsed key result
// ---------------------------------------------------------------------------

/// Metadata extracted from a GPG public key.
#[derive(Debug, Clone)]
pub struct ParsedGpgKey {
    /// Last 16 hex chars of the fingerprint (short key ID).
    pub key_id: String,
    /// Full fingerprint as uppercase hex.
    pub fingerprint: String,
    /// UID emails extracted from the key.
    pub emails: Vec<String>,
    /// Key expiry, if any.
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether the primary key has signing capability.
    pub can_sign: bool,
    /// Re-serialized public key bytes (binary `OpenPGP` format).
    pub public_key_bytes: Vec<u8>,
    /// Re-serialized ASCII-armored public key.
    pub public_key_armor: String,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse an ASCII-armored GPG public key and extract metadata.
///
/// This is CPU-intensive — call via `tokio::task::spawn_blocking` from async code.
pub fn parse_gpg_public_key(armor: &str) -> Result<ParsedGpgKey, GpgKeyError> {
    let armor = armor.trim();
    if armor.is_empty() {
        return Err(GpgKeyError::InvalidArmor);
    }

    let (key, _headers) =
        SignedPublicKey::from_string(armor).map_err(|_| GpgKeyError::InvalidArmor)?;

    // Verify self-signatures
    key.verify().map_err(|_| GpgKeyError::InvalidArmor)?;

    // Extract fingerprint
    let fp = key.fingerprint();
    let fingerprint = hex::encode(fp.as_bytes()).to_uppercase();

    // Key ID = last 16 hex chars of fingerprint
    let key_id = if fingerprint.len() >= 16 {
        fingerprint[fingerprint.len() - 16..].to_string()
    } else {
        fingerprint.clone()
    };

    // Extract emails from UIDs
    let emails = extract_uid_emails(&key);

    // Check signing capability
    let can_sign = check_can_sign(&key);

    // Expiry
    let expires_at = key.expires_at();

    // Serialize to binary bytes
    let public_key_bytes =
        pgp::ser::Serialize::to_bytes(&key).map_err(|_| GpgKeyError::MetadataError)?;

    // Re-serialize to canonical armor
    let public_key_armor = key
        .to_armored_string(pgp::composed::ArmorOptions::default())
        .map_err(|_| GpgKeyError::MetadataError)?;

    Ok(ParsedGpgKey {
        key_id,
        fingerprint,
        emails,
        expires_at,
        can_sign,
        public_key_bytes,
        public_key_armor,
    })
}

/// Extract email addresses from key UIDs.
///
/// UID format is typically `"Name <email@example.com>"`.
fn extract_uid_emails(key: &SignedPublicKey) -> Vec<String> {
    let mut emails = Vec::new();
    for user in &key.details.users {
        let uid_bytes = user.id.id();
        if let Ok(uid_str) = std::str::from_utf8(uid_bytes)
            && let Some(email) = extract_email_from_uid(uid_str)
        {
            emails.push(email);
        }
    }
    emails
}

/// Extract email from a UID string like `"Name <email@example.com>"`.
fn extract_email_from_uid(uid: &str) -> Option<String> {
    let start = uid.find('<')?;
    let end = uid.find('>')?;
    if end > start + 1 {
        Some(uid[start + 1..end].to_string())
    } else {
        None
    }
}

/// Check whether the primary key or any subkey has signing capability.
fn check_can_sign(key: &SignedPublicKey) -> bool {
    // Check primary key via its self-signatures on UIDs
    for user in &key.details.users {
        for sig in &user.signatures {
            let flags = sig.key_flags();
            if flags.sign() || flags.certify() {
                return true;
            }
        }
    }

    // Check subkeys
    for subkey in &key.public_subkeys {
        for sig in &subkey.signatures {
            let flags = sig.key_flags();
            if flags.sign() {
                return true;
            }
        }
    }

    false
}

/// Case-insensitive check: does any UID email match the user's email?
pub fn verify_email_match(key_emails: &[String], user_email: &str) -> bool {
    let user_lower = user_email.to_lowercase();
    key_emails.iter().any(|e| e.to_lowercase() == user_lower)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Real ED25519 GPG key generated for testing (UID: Test User <admin@example.com>)
    const TEST_ED25519_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEaaB09RYJKwYBBAHaRw8BAQdA7TymMz+S0gh0Y2hF6sibwc7ny6K6/1TqEWIN
zCEkavy0HVRlc3QgVXNlciA8YWRtaW5AZXhhbXBsZS5jb20+iJMEExYKADsWIQRh
2dyD0z6E1lpw/6BZs2OrURhzdQUCaaB09QIbAwULCQgHAgIiAgYVCgkICwIEFgID
AQIeBwIXgAAKCRBZs2OrURhzdWrvAP4/RbWPq4cqTCYW5AE1PykC3tPONCfZTmgQ
GbJMcvAAYQD+K9FoComHTJ3ikIjmpLswwdwi0JHTIZhhVqxm2tVsaAc=
=WQ0b
-----END PGP PUBLIC KEY BLOCK-----";

    // Real RSA GPG key generated for testing (UID: RSA Test <admin@example.com>)
    const TEST_RSA_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mQENBGmgdP0BCAC5eNWqWxkb6CdCGLb2p4Cd9SnTeLNG7LIXOE2KFvoYsiINDWUE
RdMUegWmCY/nv3aZsVqmskGJ+o0N759F/cP2JDD1fAvwkMR+YE6IbTDb9qzAKxWx
he6bag0QYacOWVDsKN++Rbx5Pd6o0kqF4Gy5FqAQ1BCENpT3NcDQbzbBELP4HJhr
4OaF0okpDSh32h8ndbfXkGkZU0QuAqteC31s/Bwe5cn6+5BpRCjNGS9SiLnS1bs7
152BsILfA22ZTKnseDBflVqJatE+TTnOjJt2rUboM8GKHxqxBNg+avJp3Ezo/3uZ
yVENku6LzkdSrRTN1kZBOmU7UJPCFoVYTwHJABEBAAG0HFJTQSBUZXN0IDxhZG1p
bkBleGFtcGxlLmNvbT6JAVEEEwEIADsWIQRZdygP3bVvKBwRqHc2ByTw8cdFtwUC
aaB0/QIbAwULCQgHAgIiAgYVCgkICwIEFgIDAQIeBwIXgAAKCRA2ByTw8cdFtw1v
B/9i0KxgnI8/Aw4+CSzJwADctpfcDrHURu2Ej2KdGYSd2+oe0lEHWHvzNPjIwolp
9ux1TSAJL2QajdJJa9FDwgv88wzKriYaAj+qEMFlpXM7nKWeNFCyjUnHXt99TuWt
JvUoqJ+rAnPegllEprhHe8+tZb42efKv+QVtBGE8WSPtocukwK0xRCyHT1CBMOWy
+ubBAufhHlQ2AfNd+LfldumuaSrgJHghX42CO+aygHuIp1awHnWqMnd/PXFWiG5E
2BXebdJ7fgSlGtOIMSCQk9uTD+7jVjqu4KWhJq2SxzdrkPLQo9jtnxrimuqjYdZH
t34e5KyD8HPtb0eMGeChvbcH
=L//0
-----END PGP PUBLIC KEY BLOCK-----";

    // GPG key with different email (UID: Other User <other@example.com>)
    #[allow(dead_code)]
    const TEST_MISMATCH_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEaaB1BRYJKwYBBAHaRw8BAQdARLy3pnfpC9xZzFm0p3C3yowaUJwkgae2DgGI
WZivJWu0Hk90aGVyIFVzZXIgPG90aGVyQGV4YW1wbGUuY29tPoiTBBMWCgA7FiEE
FxWKwBZlJmIu0Nq+boJqgnwdY00FAmmgdQUCGwMFCwkIBwICIgIGFQoJCAsCBBYC
AwECHgcCF4AACgkQboJqgnwdY00PygD+OyssgX52vWyzUQmZUXOKrGW8RT0OXfQB
LR+IPE/XK6cA/j6YvUkcTSPKKxlR8cf8PQKdl8Y/k9BqLZmX8rsNI7cG
=ivG3
-----END PGP PUBLIC KEY BLOCK-----";

    #[test]
    fn test_parse_gpg_key_valid_ed25519() {
        let result = parse_gpg_public_key(TEST_ED25519_GPG_KEY);
        assert!(result.is_ok(), "should parse valid ed25519 key: {result:?}");
        let parsed = result.unwrap();
        assert!(!parsed.fingerprint.is_empty());
        assert!(!parsed.key_id.is_empty());
        assert_eq!(parsed.key_id.len(), 16);
        assert!(parsed.emails.contains(&"admin@example.com".to_string()));
    }

    #[test]
    fn test_parse_gpg_key_valid_rsa() {
        let result = parse_gpg_public_key(TEST_RSA_GPG_KEY);
        assert!(result.is_ok(), "should parse valid RSA key: {result:?}");
        let parsed = result.unwrap();
        assert!(!parsed.fingerprint.is_empty());
        assert!(parsed.emails.contains(&"admin@example.com".to_string()));
    }

    #[test]
    fn test_parse_gpg_key_extracts_multiple_uids() {
        // Both test keys have a single UID; verify single extraction works
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        assert_eq!(parsed.emails.len(), 1);
        assert_eq!(parsed.emails[0], "admin@example.com");
    }

    #[test]
    fn test_parse_gpg_key_extracts_key_id() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        // Key ID is last 16 hex chars of fingerprint
        assert_eq!(parsed.key_id.len(), 16);
        assert!(parsed.fingerprint.ends_with(&parsed.key_id));
    }

    #[test]
    fn test_parse_gpg_key_no_expiry() {
        // Test keys generated with `0` expiry (no expiry)
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        assert!(
            parsed.expires_at.is_none(),
            "key with no expiry should return None"
        );
    }

    #[test]
    fn test_parse_gpg_key_can_sign() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        assert!(parsed.can_sign, "signing key should have can_sign=true");
    }

    #[test]
    fn test_parse_gpg_key_invalid_armor() {
        let result = parse_gpg_public_key("not a key at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gpg_key_empty_input() {
        let result = parse_gpg_public_key("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gpg_key_truncated_armor() {
        let result = parse_gpg_public_key("-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nmDME");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_email_match_exact() {
        let emails = vec!["admin@example.com".to_string()];
        assert!(verify_email_match(&emails, "admin@example.com"));
    }

    #[test]
    fn test_verify_email_match_case_insensitive() {
        let emails = vec!["Admin@Example.COM".to_string()];
        assert!(verify_email_match(&emails, "admin@example.com"));
    }

    #[test]
    fn test_verify_email_match_no_match() {
        let emails = vec!["other@example.com".to_string()];
        assert!(!verify_email_match(&emails, "admin@example.com"));
    }

    #[test]
    fn test_verify_email_match_multiple_uids_one_match() {
        let emails = vec![
            "other@example.com".to_string(),
            "admin@example.com".to_string(),
        ];
        assert!(verify_email_match(&emails, "admin@example.com"));
    }

    #[test]
    fn test_verify_email_match_empty_uids() {
        let emails: Vec<String> = vec![];
        assert!(!verify_email_match(&emails, "admin@example.com"));
    }

    #[test]
    fn test_fingerprint_format_hex() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        assert!(
            parsed.fingerprint.len() >= 40,
            "fingerprint should be at least 40 hex chars"
        );
        assert!(
            parsed.fingerprint.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should be hex: {}",
            parsed.fingerprint
        );
    }

    #[test]
    fn test_parse_gpg_key_whitespace_only() {
        let result = parse_gpg_public_key("   \n\t  ");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gpg_key_whitespace_around_valid_key() {
        // Leading/trailing whitespace should be trimmed
        let armor_with_space = format!("  \n{TEST_ED25519_GPG_KEY}\n  ");
        let result = parse_gpg_public_key(&armor_with_space);
        assert!(result.is_ok(), "whitespace around valid key should be ok");
    }

    #[test]
    fn test_gpg_key_error_display() {
        assert_eq!(
            GpgKeyError::InvalidArmor.to_string(),
            "invalid PGP public key armor"
        );
        assert_eq!(
            GpgKeyError::MetadataError.to_string(),
            "failed to extract key metadata"
        );
    }

    #[test]
    fn test_gpg_key_error_to_api_error() {
        let api_err: crate::error::ApiError = GpgKeyError::InvalidArmor.into();
        match api_err {
            crate::error::ApiError::BadRequest(msg) => {
                assert!(msg.contains("invalid PGP public key armor"));
            }
            other => panic!("expected BadRequest, got: {other:?}"),
        }
    }

    #[test]
    fn test_extract_email_from_uid_standard() {
        assert_eq!(
            extract_email_from_uid("Alice <alice@example.com>"),
            Some("alice@example.com".to_string())
        );
    }

    #[test]
    fn test_extract_email_from_uid_no_brackets() {
        assert_eq!(extract_email_from_uid("Alice alice@example.com"), None);
    }

    #[test]
    fn test_extract_email_from_uid_empty_brackets() {
        // "<>" — start=0, end=1, end > start + 1 is false → None
        assert_eq!(extract_email_from_uid("<>"), None);
    }

    #[test]
    fn test_extract_email_from_uid_only_open_bracket() {
        assert_eq!(extract_email_from_uid("Alice <alice@example.com"), None);
    }

    #[test]
    fn test_extract_email_from_uid_only_close_bracket() {
        assert_eq!(extract_email_from_uid("Alice alice@example.com>"), None);
    }

    #[test]
    fn test_extract_email_from_uid_complex_name() {
        assert_eq!(
            extract_email_from_uid("John Q. Public Jr. III <john@company.org>"),
            Some("john@company.org".to_string())
        );
    }

    #[test]
    fn test_verify_email_match_case_variations() {
        let emails = vec!["User@EXAMPLE.com".to_string()];
        assert!(verify_email_match(&emails, "user@example.com"));
        assert!(verify_email_match(&emails, "USER@EXAMPLE.COM"));
        assert!(verify_email_match(&emails, "User@EXAMPLE.com"));
    }

    #[test]
    fn test_parsed_gpg_key_debug() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        let debug = format!("{parsed:?}");
        assert!(debug.contains("ParsedGpgKey"));
        assert!(debug.contains("fingerprint"));
    }

    #[test]
    fn test_parsed_gpg_key_clone() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        let cloned = parsed.clone();
        assert_eq!(cloned.fingerprint, parsed.fingerprint);
        assert_eq!(cloned.key_id, parsed.key_id);
        assert_eq!(cloned.emails, parsed.emails);
        assert_eq!(cloned.can_sign, parsed.can_sign);
    }

    #[test]
    fn test_parse_rsa_key_can_sign() {
        let parsed = parse_gpg_public_key(TEST_RSA_GPG_KEY).unwrap();
        assert!(parsed.can_sign, "RSA signing key should have can_sign=true");
    }

    #[test]
    fn test_parse_rsa_key_key_id_length() {
        let parsed = parse_gpg_public_key(TEST_RSA_GPG_KEY).unwrap();
        assert_eq!(parsed.key_id.len(), 16);
        assert!(parsed.fingerprint.ends_with(&parsed.key_id));
    }

    #[test]
    fn test_parse_rsa_key_has_public_key_bytes() {
        let parsed = parse_gpg_public_key(TEST_RSA_GPG_KEY).unwrap();
        assert!(
            !parsed.public_key_bytes.is_empty(),
            "RSA key should have non-empty serialized bytes"
        );
    }

    #[test]
    fn test_parse_rsa_key_has_armor() {
        let parsed = parse_gpg_public_key(TEST_RSA_GPG_KEY).unwrap();
        assert!(
            parsed
                .public_key_armor
                .contains("-----BEGIN PGP PUBLIC KEY BLOCK-----"),
            "RSA key armor should contain BEGIN marker"
        );
        assert!(
            parsed
                .public_key_armor
                .contains("-----END PGP PUBLIC KEY BLOCK-----"),
            "RSA key armor should contain END marker"
        );
    }

    #[test]
    fn test_verify_email_match_special_chars_in_email() {
        let emails = vec!["user+tag@sub.example.com".to_string()];
        assert!(verify_email_match(&emails, "user+tag@sub.example.com"));
        assert!(verify_email_match(&emails, "USER+TAG@SUB.EXAMPLE.COM"));
    }

    #[test]
    fn test_extract_email_from_uid_multiple_brackets() {
        // Should take the first pair of < >
        let result = extract_email_from_uid("Name <first@e.com> <second@e.com>");
        assert_eq!(result, Some("first@e.com".to_string()));
    }

    #[test]
    fn test_extract_email_from_uid_empty_string() {
        assert_eq!(extract_email_from_uid(""), None);
    }

    #[test]
    fn test_parse_gpg_key_serialized_bytes_not_empty() {
        let parsed = parse_gpg_public_key(TEST_ED25519_GPG_KEY).unwrap();
        assert!(!parsed.public_key_bytes.is_empty());
        assert!(!parsed.public_key_armor.is_empty());
        assert!(
            parsed
                .public_key_armor
                .contains("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        );
    }
}
