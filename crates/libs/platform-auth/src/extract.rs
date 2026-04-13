// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

pub fn extract_bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    Some(token)
}

pub fn extract_session_cookie(parts: &Parts) -> Option<&str> {
    let cookies = parts
        .headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?;
    for cookie in cookies.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix("session=")
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

pub fn extract_ip(
    parts: &Parts,
    trust_proxy: bool,
    trust_proxy_cidrs: &[String],
) -> Option<String> {
    if trust_proxy
        && let Some(forwarded) = parts.headers.get("x-forwarded-for")
        && let Ok(val) = forwarded.to_str()
        && let Some(first_ip) = val.split(',').next()
    {
        let ip_str = first_ip.trim();
        if !trust_proxy_cidrs.is_empty() {
            let connecting_ip = parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip());
            if let Some(conn_ip) = connecting_ip
                && !cidr_matches(conn_ip, trust_proxy_cidrs)
            {
                return Some(conn_ip.to_string());
            }
        }
        return Some(ip_str.to_owned());
    }
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
}

/// Check whether an IP address matches any of the configured trusted CIDRs.
pub fn cidr_matches(ip: std::net::IpAddr, cidrs: &[String]) -> bool {
    cidrs.iter().any(|cidr_str| {
        cidr_str
            .parse::<ipnetwork::IpNetwork>()
            .is_ok_and(|net| net.contains(ip))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn make_parts(headers: &[(&str, &str)]) -> Parts {
        let mut builder = Request::builder().uri("/test");
        for &(k, v) in headers {
            builder = builder.header(k, v);
        }
        let (parts, ()) = builder.body(()).unwrap().into_parts();
        parts
    }

    #[test]
    fn bearer_token_valid() {
        let parts = make_parts(&[("authorization", "Bearer abc123")]);
        assert_eq!(extract_bearer_token(&parts), Some("abc123"));
    }

    #[test]
    fn bearer_token_missing_header() {
        let parts = make_parts(&[]);
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn bearer_token_wrong_scheme() {
        let parts = make_parts(&[("authorization", "Basic dXNlcjpwYXNz")]);
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn bearer_token_empty_after_prefix() {
        let parts = make_parts(&[("authorization", "Bearer ")]);
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn session_cookie_valid() {
        let parts = make_parts(&[("cookie", "session=tok123")]);
        assert_eq!(extract_session_cookie(&parts), Some("tok123"));
    }

    #[test]
    fn session_cookie_among_others() {
        let parts = make_parts(&[("cookie", "foo=bar; session=tok123; baz=qux")]);
        assert_eq!(extract_session_cookie(&parts), Some("tok123"));
    }

    #[test]
    fn session_cookie_missing() {
        let parts = make_parts(&[("cookie", "foo=bar; other=val")]);
        assert_eq!(extract_session_cookie(&parts), None);
    }

    #[test]
    fn session_cookie_empty_value() {
        let parts = make_parts(&[("cookie", "session=")]);
        assert_eq!(extract_session_cookie(&parts), None);
    }

    #[test]
    fn ip_from_forwarded_for_trusted() {
        let parts = make_parts(&[("x-forwarded-for", "1.2.3.4, 5.6.7.8")]);
        assert_eq!(extract_ip(&parts, true, &[]), Some("1.2.3.4".into()));
    }

    #[test]
    fn ip_forwarded_for_ignored_when_not_trusted() {
        let parts = make_parts(&[("x-forwarded-for", "1.2.3.4")]);
        assert_eq!(extract_ip(&parts, false, &[]), None);
    }

    #[test]
    fn ip_from_connect_info() {
        let mut parts = make_parts(&[]);
        let addr: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
        parts.extensions.insert(axum::extract::ConnectInfo(addr));
        assert_eq!(extract_ip(&parts, false, &[]), Some("127.0.0.1".into()));
    }

    #[test]
    fn cidr_matches_valid_cidr() {
        let ip: std::net::IpAddr = "10.1.2.3".parse().unwrap();
        assert!(cidr_matches(ip, &["10.0.0.0/8".to_string()]));
    }

    #[test]
    fn cidr_matches_outside_range() {
        let ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!cidr_matches(ip, &["10.0.0.0/8".to_string()]));
    }

    #[test]
    fn cidr_matches_empty_list() {
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!cidr_matches(ip, &[]));
    }

    #[test]
    fn cidr_matches_invalid_cidr_skipped() {
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!cidr_matches(ip, &["not-a-cidr".to_string()]));
    }

    #[test]
    fn cidr_matches_multiple_ranges() {
        let ip: std::net::IpAddr = "172.16.5.1".parse().unwrap();
        assert!(cidr_matches(
            ip,
            &["10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()]
        ));
    }

    #[test]
    fn extract_ip_trust_proxy_with_cidr_match() {
        let mut parts = make_parts(&[("x-forwarded-for", "1.2.3.4, 5.6.7.8")]);
        let addr: std::net::SocketAddr = "10.0.0.1:9000".parse().unwrap();
        parts.extensions.insert(axum::extract::ConnectInfo(addr));
        // Connecting IP (10.0.0.1) is in the trusted CIDR → use x-forwarded-for
        let cidrs = vec!["10.0.0.0/8".to_string()];
        assert_eq!(extract_ip(&parts, true, &cidrs), Some("1.2.3.4".into()));
    }

    #[test]
    fn extract_ip_trust_proxy_with_cidr_no_match() {
        let mut parts = make_parts(&[("x-forwarded-for", "1.2.3.4, 5.6.7.8")]);
        let addr: std::net::SocketAddr = "192.168.1.1:9000".parse().unwrap();
        parts.extensions.insert(axum::extract::ConnectInfo(addr));
        // Connecting IP (192.168.1.1) is NOT in the trusted CIDR → use ConnectInfo
        let cidrs = vec!["10.0.0.0/8".to_string()];
        assert_eq!(extract_ip(&parts, true, &cidrs), Some("192.168.1.1".into()));
    }
}
