// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! SSRF protection for webhook URLs.
//!
//! Blocks private IPs, link-local, loopback, cloud metadata endpoints, and
//! non-HTTP schemes. This is intentionally a standalone module so it can be
//! used without pulling in `ApiError` or other HTTP types.

use std::net::IpAddr;

/// Validate a URL against SSRF attacks. Returns `Ok(())` if the URL is safe.
///
/// Blocks:
/// - Private IPs (10/8, 172.16/12, 192.168/16, 127/8)
/// - Link-local (169.254/16)
/// - Loopback (`::1`, localhost)
/// - Cloud metadata (169.254.169.254, metadata.google.internal)
/// - Non-HTTP schemes (ftp://, file://, etc.)
/// - IPv4-mapped IPv6 addresses pointing to private ranges
pub fn validate_webhook_url(url_str: &str) -> Result<(), SsrfError> {
    let parsed = url::Url::parse(url_str).map_err(|_| SsrfError::InvalidUrl)?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SsrfError::BadScheme);
    }

    let host = parsed.host_str().ok_or(SsrfError::NoHost)?;

    // Block well-known dangerous hostnames
    let blocked_hosts = [
        "localhost",
        "169.254.169.254",
        "metadata.google.internal",
        "[::1]",
    ];
    let host_lower = host.to_lowercase();
    if blocked_hosts.iter().any(|b| host_lower == *b) {
        return Err(SsrfError::BlockedHost);
    }

    // Block private/reserved IPs (strip brackets for IPv6 literals like [::1])
    let bare_ip = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = bare_ip.parse::<IpAddr>()
        && is_private_ip(ip)
    {
        return Err(SsrfError::PrivateIp);
    }

    Ok(())
}

/// Check whether an IP address is private/reserved/loopback.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => {
            // Check IPv4-mapped IPv6 addresses (::ffff:x.x.x.x) — SSRF bypass vector
            if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                return is_private_ipv4(mapped_v4);
            }
            v6.is_loopback()          // ::1
                || v6.is_unspecified() // ::
                || is_ipv6_unique_local(&v6)  // fc00::/7 (includes fd00::/8)
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()          // 127.0.0.0/8
        || v4.is_private()    // 10/8, 172.16/12, 192.168/16
        || v4.is_link_local() // 169.254/16
        || v4.is_broadcast()  // 255.255.255.255
        || v4.is_unspecified() // 0.0.0.0
}

fn is_ipv6_unique_local(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// Errors from SSRF URL validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsrfError {
    InvalidUrl,
    BadScheme,
    NoHost,
    BlockedHost,
    PrivateIp,
}

impl std::fmt::Display for SsrfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl => write!(f, "invalid URL"),
            Self::BadScheme => write!(f, "webhook URL must use http or https"),
            Self::NoHost => write!(f, "webhook URL must have a host"),
            Self::BlockedHost => {
                write!(f, "webhook URL must not target internal/metadata endpoints")
            }
            Self::PrivateIp => write!(
                f,
                "webhook URL must not target private/reserved IP addresses"
            ),
        }
    }
}

impl std::error::Error for SsrfError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_https() {
        assert!(validate_webhook_url("https://example.com/hook").is_ok());
    }

    #[test]
    fn allows_public_http() {
        assert!(validate_webhook_url("http://example.com/hook").is_ok());
    }

    #[test]
    fn blocks_localhost() {
        assert_eq!(
            validate_webhook_url("http://localhost:8080/hook"),
            Err(SsrfError::BlockedHost)
        );
    }

    #[test]
    fn blocks_metadata() {
        assert_eq!(
            validate_webhook_url("http://169.254.169.254/latest/"),
            Err(SsrfError::BlockedHost)
        );
    }

    #[test]
    fn blocks_private_ip() {
        assert_eq!(
            validate_webhook_url("http://10.0.0.1/hook"),
            Err(SsrfError::PrivateIp)
        );
    }

    #[test]
    fn blocks_loopback_v6() {
        assert_eq!(
            validate_webhook_url("http://[::1]:8080/hook"),
            Err(SsrfError::BlockedHost)
        );
    }

    #[test]
    fn blocks_ftp() {
        assert_eq!(
            validate_webhook_url("ftp://example.com/file"),
            Err(SsrfError::BadScheme)
        );
    }

    #[test]
    fn blocks_invalid_url() {
        assert_eq!(
            validate_webhook_url("not a url"),
            Err(SsrfError::InvalidUrl)
        );
    }

    #[test]
    fn blocks_google_metadata() {
        assert_eq!(
            validate_webhook_url("http://metadata.google.internal/computeMetadata/v1/"),
            Err(SsrfError::BlockedHost)
        );
    }

    #[test]
    fn blocks_192_168() {
        assert_eq!(
            validate_webhook_url("http://192.168.1.1/hook"),
            Err(SsrfError::PrivateIp)
        );
    }

    #[test]
    fn blocks_172_16() {
        assert_eq!(
            validate_webhook_url("http://172.16.0.1/hook"),
            Err(SsrfError::PrivateIp)
        );
    }

    #[test]
    fn blocks_link_local() {
        assert_eq!(
            validate_webhook_url("http://169.254.1.1/hook"),
            Err(SsrfError::PrivateIp)
        );
    }
}
