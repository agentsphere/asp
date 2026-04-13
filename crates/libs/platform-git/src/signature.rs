// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Commit signature parsing and GPG signature verification.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureStatus {
    Verified,
    UnverifiedSigner,
    BadSignature,
    NoSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureInfo {
    pub status: SignatureStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_name: Option<String>,
}

/// Parsed GPG signature from a commit object.
pub struct ParsedCommitSignature {
    pub signature_armor: String,
    pub signed_data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Commit object parsing
// ---------------------------------------------------------------------------

/// Parse a raw commit object (from `git cat-file commit <sha>`) and extract
/// the GPG signature and the data that was signed.
///
/// Returns `None` if the commit has no `gpgsig` header.
pub fn parse_commit_gpgsig(raw_commit: &[u8]) -> Option<ParsedCommitSignature> {
    let text = String::from_utf8_lossy(raw_commit);

    // Find the gpgsig header
    let gpgsig_start = text.find("\ngpgsig ")?;
    let header_start = gpgsig_start + 1; // skip the leading \n

    // Extract the signature (first line after "gpgsig " + continuation lines starting with " ")
    let after_tag = &text[header_start + "gpgsig ".len()..];
    let mut sig_lines = Vec::new();

    for line in after_tag.lines() {
        if sig_lines.is_empty() {
            sig_lines.push(line.to_owned());
        } else if let Some(continuation) = line.strip_prefix(' ') {
            sig_lines.push(continuation.to_owned());
        } else {
            break;
        }
    }

    let signature_armor = sig_lines.join("\n");

    // Reconstruct signed data: the commit object with the gpgsig header removed
    let mut signed_data = String::new();
    let mut in_gpgsig = false;
    for line in text.lines() {
        if line.starts_with("gpgsig ") {
            in_gpgsig = true;
            continue;
        }
        if in_gpgsig {
            if line.starts_with(' ') {
                continue;
            }
            in_gpgsig = false;
        }
        if !signed_data.is_empty() {
            signed_data.push('\n');
        }
        signed_data.push_str(line);
    }
    // Git's commit object ends with a trailing newline
    signed_data.push('\n');

    Some(ParsedCommitSignature {
        signature_armor,
        signed_data: signed_data.into_bytes(),
    })
}

/// Extract the signing key ID from a PGP signature armor.
///
/// Returns the hex key ID of the issuer, or `None` if parsing fails.
pub fn extract_signing_key_id(signature_armor: &str) -> Option<String> {
    use pgp::composed::{Deserializable, StandaloneSignature};

    let (sig, _) = StandaloneSignature::from_string(signature_armor).ok()?;
    let issuers = sig.signature.issuer();
    issuers
        .first()
        .map(|id| hex::encode(id.as_ref()).to_uppercase())
}

/// Verify a detached PGP signature against the given data and public key.
///
/// Returns `true` if the signature is valid for the given data and key.
pub fn verify_signature(
    signature_armor: &str,
    signed_data: &[u8],
    public_key: &pgp::composed::SignedPublicKey,
) -> bool {
    use pgp::composed::{Deserializable, StandaloneSignature};

    let Ok((sig, _)) = StandaloneSignature::from_string(signature_armor) else {
        return false;
    };

    sig.signature
        .verify(public_key, std::io::Cursor::new(signed_data))
        .is_ok()
}

/// Validate that a string looks like a valid commit SHA (7-64 hex characters).
pub fn validate_commit_sha(sha: &str) -> bool {
    let len = sha.len();
    (7..=64).contains(&len) && sha.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_COMMIT_WITH_SIG: &str = "tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39\nauthor Test User <test@example.com> 1708900000 +0000\ncommitter Test User <test@example.com> 1708900000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iHUEABYKAB0WIQRQVMGx5eUIA0cYisgMFfJwj0kygAUCaaB6RQAKCRA=\n =abcd\n -----END PGP SIGNATURE-----\n\nInitial commit\n";

    const RAW_COMMIT_NO_SIG: &str = "tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39\nauthor Test User <test@example.com> 1708900000 +0000\ncommitter Test User <test@example.com> 1708900000 +0000\n\nInitial commit\n";

    // Real GPG detached signature from an ed25519-signed git commit.
    const TEST_SIGNATURE_ARMOR: &str = "-----BEGIN PGP SIGNATURE-----\n\niIgEABYKADAWIQTmKamlVAJv9iMlEMrRmES3Hv392gUCaaE6tBIcYWRtaW5AZXhh\nbXBsZS5jb20ACgkQ0ZhEtx79/dpnaQD/Z8aJcamYlCw8M1wYPQ2cs707fMU/0ZlX\nL5yWRQMrxvAA/0C0VEWbRpA0Cy5oknO4BGmq5qp5WWOOIm/66OKLZQMF\n=ZaF0\n-----END PGP SIGNATURE-----";

    const TEST_SIGNER_PUBLIC_KEY: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nmDMEaaE6tBYJKwYBBAHaRw8BAQdAfjHN7arMA/6FCc6HMDgDSdP5YSuuPgcUf0MX\nsqOE99a0H1Rlc3QgU2lnbmVyIDxhZG1pbkBleGFtcGxlLmNvbT6IkwQTFgoAOxYh\nBOYpqaVUAm/2IyUQytGYRLce/f3aBQJpoTq0AhsDBQsJCAcCAiICBhUKCQgLAgQW\nAgMBAh4HAheAAAoJENGYRLce/f3aV7UBAJLgQGxEoWY/3ISBHmJxhVgNYJCjSC2Z\ntCQTVVkW5N9mAQCdWcF33bG8ZUu/J1n00XHHY4OgrsuY0mVnMpwHVntiDA==\n=V+C4\n-----END PGP PUBLIC KEY BLOCK-----";

    const TEST_SIGNED_RAW_COMMIT: &str = "tree 26f0a7f39487d471fe50def407139827d3ce29b9\nauthor Test Signer <admin@example.com> 1772174004 +0100\ncommitter Test Signer <admin@example.com> 1772174004 +0100\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iIgEABYKADAWIQTmKamlVAJv9iMlEMrRmES3Hv392gUCaaE6tBIcYWRtaW5AZXhh\n bXBsZS5jb20ACgkQ0ZhEtx79/dpnaQD/Z8aJcamYlCw8M1wYPQ2cs707fMU/0ZlX\n L5yWRQMrxvAA/0C0VEWbRpA0Cy5oknO4BGmq5qp5WWOOIm/66OKLZQMF\n =ZaF0\n -----END PGP SIGNATURE-----\n\nSigned commit\n";

    #[test]
    fn test_parse_commit_object_with_gpgsig() {
        let result = parse_commit_gpgsig(RAW_COMMIT_WITH_SIG.as_bytes());
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert!(parsed.signature_armor.contains("BEGIN PGP SIGNATURE"));
        assert!(!parsed.signed_data.is_empty());
    }

    #[test]
    fn test_parse_commit_object_without_gpgsig() {
        let result = parse_commit_gpgsig(RAW_COMMIT_NO_SIG.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_reconstruct_signed_data() {
        let result = parse_commit_gpgsig(RAW_COMMIT_WITH_SIG.as_bytes()).unwrap();
        let signed_text = String::from_utf8_lossy(&result.signed_data);
        assert!(!signed_text.contains("gpgsig"));
        assert!(signed_text.contains("tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39"));
        assert!(signed_text.contains("Initial commit"));
    }

    #[test]
    fn test_signature_status_serialization() {
        assert_eq!(
            serde_json::to_string(&SignatureStatus::Verified).unwrap(),
            r#""verified""#
        );
        assert_eq!(
            serde_json::to_string(&SignatureStatus::NoSignature).unwrap(),
            r#""no_signature""#
        );
        assert_eq!(
            serde_json::to_string(&SignatureStatus::BadSignature).unwrap(),
            r#""bad_signature""#
        );
        assert_eq!(
            serde_json::to_string(&SignatureStatus::UnverifiedSigner).unwrap(),
            r#""unverified_signer""#
        );
    }

    #[test]
    fn test_signature_info_none_fields_omitted() {
        let info = SignatureInfo {
            status: SignatureStatus::NoSignature,
            signer_key_id: None,
            signer_fingerprint: None,
            signer_name: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("signer_key_id").is_none());
    }

    #[test]
    fn test_signature_status_serde_roundtrip() {
        for status in &[
            SignatureStatus::Verified,
            SignatureStatus::UnverifiedSigner,
            SignatureStatus::BadSignature,
            SignatureStatus::NoSignature,
        ] {
            let json = serde_json::to_string(status).unwrap();
            let parsed: SignatureStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, parsed);
        }
    }

    #[test]
    fn test_extract_signing_key_id_valid() {
        let key_id = extract_signing_key_id(TEST_SIGNATURE_ARMOR);
        assert!(key_id.is_some());
        assert_eq!(key_id.unwrap(), "D19844B71EFDFDDA");
    }

    #[test]
    fn test_extract_signing_key_id_invalid() {
        assert!(extract_signing_key_id("not a valid signature").is_none());
        assert!(extract_signing_key_id("").is_none());
    }

    #[test]
    fn test_verify_signature_valid() {
        use pgp::composed::{Deserializable, SignedPublicKey};
        let (key, _) = SignedPublicKey::from_string(TEST_SIGNER_PUBLIC_KEY).unwrap();
        let parsed = parse_commit_gpgsig(TEST_SIGNED_RAW_COMMIT.as_bytes()).unwrap();
        assert!(verify_signature(
            &parsed.signature_armor,
            &parsed.signed_data,
            &key
        ));
    }

    #[test]
    fn test_verify_signature_tampered_data() {
        use pgp::composed::{Deserializable, SignedPublicKey};
        let (key, _) = SignedPublicKey::from_string(TEST_SIGNER_PUBLIC_KEY).unwrap();
        let parsed = parse_commit_gpgsig(TEST_SIGNED_RAW_COMMIT.as_bytes()).unwrap();
        let mut tampered = parsed.signed_data.clone();
        tampered[0] = b'X';
        assert!(!verify_signature(&parsed.signature_armor, &tampered, &key));
    }

    #[test]
    fn test_verify_signature_invalid_armor() {
        use pgp::composed::{Deserializable, SignedPublicKey};
        let (key, _) = SignedPublicKey::from_string(TEST_SIGNER_PUBLIC_KEY).unwrap();
        assert!(!verify_signature("not a valid signature", b"data", &key));
    }

    #[test]
    fn test_validate_commit_sha_valid() {
        assert!(validate_commit_sha("abc1234"));
        assert!(validate_commit_sha(
            "abc1234567890abcdef1234567890abcdef123456"
        ));
        assert!(validate_commit_sha(&"a".repeat(64)));
    }

    #[test]
    fn test_validate_commit_sha_invalid() {
        assert!(!validate_commit_sha("abc12")); // too short
        assert!(!validate_commit_sha("")); // empty
        assert!(!validate_commit_sha("ghijkl1")); // non-hex
        assert!(!validate_commit_sha(&"a".repeat(65))); // too long
    }

    #[test]
    fn test_parse_commit_gpgsig_empty() {
        assert!(parse_commit_gpgsig(b"").is_none());
    }

    #[test]
    fn test_signed_data_ends_with_newline() {
        let result = parse_commit_gpgsig(RAW_COMMIT_WITH_SIG.as_bytes()).unwrap();
        let signed = String::from_utf8(result.signed_data).unwrap();
        assert!(signed.ends_with('\n'));
    }
}
