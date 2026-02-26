use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SignatureStatus {
    Verified,
    UnverifiedSigner,
    BadSignature,
    NoSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SignatureInfo {
    pub status: SignatureStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Commit object parsing
// ---------------------------------------------------------------------------

/// Parsed GPG signature from a commit object.
pub struct ParsedCommitSignature {
    pub signature_armor: String,
    pub signed_data: Vec<u8>,
}

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
            // First line of the signature
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

    // A raw commit object with a GPG signature (from a real git repo)
    const RAW_COMMIT_WITH_SIG: &str = "tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39\nauthor Test User <test@example.com> 1708900000 +0000\ncommitter Test User <test@example.com> 1708900000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iHUEABYKAB0WIQRQVMGx5eUIA0cYisgMFfJwj0kygAUCaaB6RQAKCRA=\n =abcd\n -----END PGP SIGNATURE-----\n\nInitial commit\n";

    const RAW_COMMIT_NO_SIG: &str = "tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39\nauthor Test User <test@example.com> 1708900000 +0000\ncommitter Test User <test@example.com> 1708900000 +0000\n\nInitial commit\n";

    #[test]
    fn test_parse_commit_object_with_gpgsig() {
        let result = parse_commit_gpgsig(RAW_COMMIT_WITH_SIG.as_bytes());
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert!(parsed.signature_armor.contains("BEGIN PGP SIGNATURE"));
        assert!(parsed.signature_armor.contains("END PGP SIGNATURE"));
        assert!(!parsed.signed_data.is_empty());
    }

    #[test]
    fn test_parse_commit_object_without_gpgsig() {
        let result = parse_commit_gpgsig(RAW_COMMIT_NO_SIG.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_commit_object_multiline_gpgsig() {
        let raw = "tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39\nauthor Test <t@e.com> 1708900000 +0000\ncommitter Test <t@e.com> 1708900000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n line1data\n line2data\n line3data\n =checksum\n -----END PGP SIGNATURE-----\n\nTest\n";

        let result = parse_commit_gpgsig(raw.as_bytes());
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert!(parsed.signature_armor.contains("line1data"));
        assert!(parsed.signature_armor.contains("line2data"));
        assert!(parsed.signature_armor.contains("line3data"));
    }

    #[test]
    fn test_reconstruct_signed_data() {
        let result = parse_commit_gpgsig(RAW_COMMIT_WITH_SIG.as_bytes()).unwrap();
        let signed_text = String::from_utf8_lossy(&result.signed_data);
        // Signed data must NOT contain the gpgsig header
        assert!(!signed_text.contains("gpgsig"));
        assert!(!signed_text.contains("BEGIN PGP SIGNATURE"));
        // But must contain the rest of the commit
        assert!(signed_text.contains("tree 4b825dc642cb6eb9a060e54bf899d69a6a8e3b39"));
        assert!(signed_text.contains("author Test User"));
        assert!(signed_text.contains("Initial commit"));
    }

    #[test]
    fn test_signature_status_serialization_verified() {
        let json = serde_json::to_string(&SignatureStatus::Verified).unwrap();
        assert_eq!(json, r#""verified""#);
    }

    #[test]
    fn test_signature_status_serialization_no_signature() {
        let json = serde_json::to_string(&SignatureStatus::NoSignature).unwrap();
        assert_eq!(json, r#""no_signature""#);
    }

    #[test]
    fn test_signature_status_serialization_bad_signature() {
        let json = serde_json::to_string(&SignatureStatus::BadSignature).unwrap();
        assert_eq!(json, r#""bad_signature""#);
    }

    #[test]
    fn test_signature_status_serialization_unverified_signer() {
        let json = serde_json::to_string(&SignatureStatus::UnverifiedSigner).unwrap();
        assert_eq!(json, r#""unverified_signer""#);
    }

    #[test]
    fn test_signature_info_serialization() {
        let info = SignatureInfo {
            status: SignatureStatus::Verified,
            signer_key_id: Some("ABCDEF01".into()),
            signer_fingerprint: Some("DEADBEEF".into()),
            signer_name: Some("Alice".into()),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["status"], "verified");
        assert_eq!(json["signer_key_id"], "ABCDEF01");
        assert_eq!(json["signer_fingerprint"], "DEADBEEF");
        assert_eq!(json["signer_name"], "Alice");
    }

    #[test]
    fn test_signature_info_serialization_none_fields_omitted() {
        let info = SignatureInfo {
            status: SignatureStatus::NoSignature,
            signer_key_id: None,
            signer_fingerprint: None,
            signer_name: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["status"], "no_signature");
        assert!(json.get("signer_key_id").is_none());
        assert!(json.get("signer_fingerprint").is_none());
        assert!(json.get("signer_name").is_none());
    }

    #[test]
    fn test_extract_signing_key_id_invalid_signature() {
        let result = extract_signing_key_id("not a valid signature");
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_commit_sha_valid() {
        assert!(validate_commit_sha("abc1234"));
        assert!(validate_commit_sha(
            "abc1234567890abcdef1234567890abcdef123456"
        ));
        // 64 hex chars (SHA-256)
        assert!(validate_commit_sha(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn test_validate_commit_sha_invalid() {
        assert!(!validate_commit_sha("abc12")); // too short
        assert!(!validate_commit_sha("")); // empty
        assert!(!validate_commit_sha("ghijkl1")); // non-hex
        // 65 hex chars (too long)
        assert!(!validate_commit_sha(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn test_commit_info_with_signature_field() {
        use crate::git::browser::CommitInfo;
        let commit = CommitInfo {
            sha: "abc123".into(),
            message: "test".into(),
            author_name: "Alice".into(),
            author_email: "alice@example.com".into(),
            authored_at: "2026-02-26T00:00:00Z".into(),
            committer_name: "Alice".into(),
            committer_email: "alice@example.com".into(),
            committed_at: "2026-02-26T00:00:00Z".into(),
            signature: Some(SignatureInfo {
                status: SignatureStatus::Verified,
                signer_key_id: Some("ABC123".into()),
                signer_fingerprint: None,
                signer_name: None,
            }),
        };
        let json = serde_json::to_value(&commit).unwrap();
        assert_eq!(json["signature"]["status"], "verified");
    }

    #[test]
    fn test_extract_author_email_from_commit() {
        let raw = b"tree abc\nauthor Alice <alice@example.com> 1708900000 +0000\ncommitter Bob <bob@example.com> 1708900000 +0000\n\ntest\n";
        let email = crate::git::browser::extract_author_email_from_commit(raw);
        assert_eq!(email, Some("alice@example.com".to_owned()));
    }
}
