use super::error::RegistryError;

/// A parsed and validated content digest (sha256 only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub algorithm: String,
    pub hex: String,
}

impl Digest {
    /// Parse a digest string like "sha256:abcdef0123456789...".
    pub fn parse(s: &str) -> Result<Self, RegistryError> {
        let (algo, hex) = s
            .split_once(':')
            .ok_or_else(|| RegistryError::DigestInvalid("missing ':' separator".into()))?;

        if algo != "sha256" {
            return Err(RegistryError::DigestInvalid(format!(
                "unsupported algorithm: {algo}"
            )));
        }

        if hex.len() != 64 {
            return Err(RegistryError::DigestInvalid(format!(
                "sha256 hex must be 64 chars, got {}",
                hex.len()
            )));
        }

        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RegistryError::DigestInvalid(
                "hex contains non-hex characters".into(),
            ));
        }

        // Normalize to lowercase
        let hex = hex.to_ascii_lowercase();

        Ok(Self {
            algorithm: algo.to_string(),
            hex,
        })
    }

    /// The full digest string: "sha256:abcdef..."
    pub fn as_str(&self) -> String {
        format!("{}:{}", self.algorithm, self.hex)
    }

    /// `MinIO` object path for this blob.
    pub fn minio_path(&self) -> String {
        format!("registry/blobs/{}/{}", self.algorithm, self.hex)
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.hex)
    }
}

/// Compute the sha256 digest of the given bytes.
pub fn sha256_digest(data: &[u8]) -> Digest {
    use sha2::Digest as _;
    let hash = sha2::Sha256::digest(data);
    Digest {
        algorithm: "sha256".into(),
        hex: hex::encode(hash),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_sha256() {
        let hex = "a".repeat(64);
        let input = format!("sha256:{hex}");
        let d = Digest::parse(&input).unwrap();
        assert_eq!(d.algorithm, "sha256");
        assert_eq!(d.hex, hex);
    }

    #[test]
    fn parse_normalizes_to_lowercase() {
        let hex = "A".repeat(64);
        let input = format!("sha256:{hex}");
        let d = Digest::parse(&input).unwrap();
        assert_eq!(d.hex, "a".repeat(64));
    }

    #[test]
    fn parse_rejects_missing_colon() {
        assert!(Digest::parse("sha256abc").is_err());
    }

    #[test]
    fn parse_rejects_unsupported_algorithm() {
        let hex = "a".repeat(64);
        assert!(Digest::parse(&format!("md5:{hex}")).is_err());
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(Digest::parse("sha256:abcdef").is_err());
    }

    #[test]
    fn parse_rejects_non_hex() {
        let input = format!("sha256:{}", "g".repeat(64));
        assert!(Digest::parse(&input).is_err());
    }

    #[test]
    fn minio_path_format() {
        let hex = "a".repeat(64);
        let d = Digest::parse(&format!("sha256:{hex}")).unwrap();
        assert_eq!(d.minio_path(), format!("registry/blobs/sha256/{hex}"));
    }

    #[test]
    fn display_format() {
        let hex = "b".repeat(64);
        let d = Digest::parse(&format!("sha256:{hex}")).unwrap();
        assert_eq!(d.to_string(), format!("sha256:{hex}"));
    }

    #[test]
    fn sha256_digest_computes() {
        let d = sha256_digest(b"hello");
        assert_eq!(d.algorithm, "sha256");
        assert_eq!(d.hex.len(), 64);
        // Known SHA256 of "hello"
        assert_eq!(
            d.hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn as_str_roundtrip() {
        let hex = "c".repeat(64);
        let d = Digest::parse(&format!("sha256:{hex}")).unwrap();
        let s = d.as_str();
        let d2 = Digest::parse(&s).unwrap();
        assert_eq!(d, d2);
    }
}
