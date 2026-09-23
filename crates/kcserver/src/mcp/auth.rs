//
// auth.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Guards every request that reaches the MCP listener.
//!
//! The listener binds loopback only, but a browser on the same machine can
//! still reach it, so `Host` and `Origin` are checked to satisfy the MCP
//! specification's DNS-rebinding requirements before the bearer token is
//! looked up.

use hyper::header::{HeaderMap, AUTHORIZATION, HOST, ORIGIN};

/// Why a request was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthRejection {
    /// The `Host` or `Origin` header did not name a loopback address.
    Forbidden(String),

    /// No usable bearer token was presented.
    Unauthorized(String),
}

/// Apply the loopback guards and return the bearer token the request presented.
///
/// Resolving that token to a frontend is the caller's job, so this stays
/// independent of the registry's locking.
pub fn check_request(headers: &HeaderMap) -> Result<&str, AuthRejection> {
    check_loopback(headers)?;
    bearer_token(headers)
        .ok_or_else(|| AuthRejection::Unauthorized("No bearer token supplied".to_string()))
}

/// Apply the loopback guards alone, for the one request that carries no token:
/// the server card, which exists to be read before a client has one.
pub fn check_loopback(headers: &HeaderMap) -> Result<(), AuthRejection> {
    if let Some(host) = headers.get(HOST) {
        let host = host
            .to_str()
            .map_err(|_| AuthRejection::Forbidden("Host header is not valid text".to_string()))?;
        if !is_loopback_authority(host) {
            return Err(AuthRejection::Forbidden(format!(
                "Host '{}' is not a loopback address",
                host
            )));
        }
    }

    if let Some(origin) = headers.get(ORIGIN) {
        let origin = origin
            .to_str()
            .map_err(|_| AuthRejection::Forbidden("Origin header is not valid text".to_string()))?;
        if !is_loopback_origin(origin) {
            return Err(AuthRejection::Forbidden(format!(
                "Origin '{}' is not a loopback origin",
                origin
            )));
        }
    }

    Ok(())
}

/// Extract the token from an `Authorization: Bearer <token>` header.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Whether a `Host` header value names the local machine.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.rsplit_once(':') {
        // An IPv6 literal keeps its brackets; a bare `[::1]` has no port.
        Some((host, port)) if !host.ends_with('[') && port.chars().all(|c| c.is_ascii_digit()) => {
            host
        }
        _ => authority,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');

    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(addr) => addr.is_loopback(),
        Err(_) => false,
    }
}

/// Whether an `Origin` header value names a loopback origin.
fn is_loopback_origin(origin: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    is_loopback_authority(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    fn headers(pairs: &[(hyper::header::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn accepts_loopback_hosts_and_origins() {
        let map = headers(&[
            (HOST, "127.0.0.1:5000"),
            (ORIGIN, "http://localhost:5000"),
            (AUTHORIZATION, "Bearer abc"),
        ]);
        assert_eq!(check_request(&map), Ok("abc"));
    }

    #[test]
    fn accepts_ipv6_loopback_host() {
        let map = headers(&[(HOST, "[::1]:5000"), (AUTHORIZATION, "Bearer abc")]);
        assert_eq!(check_request(&map), Ok("abc"));
    }

    #[test]
    fn rejects_non_loopback_host() {
        let map = headers(&[(HOST, "evil.example.com"), (AUTHORIZATION, "Bearer abc")]);
        assert!(matches!(
            check_request(&map),
            Err(AuthRejection::Forbidden(_))
        ));
    }

    #[test]
    fn rejects_non_loopback_origin() {
        let map = headers(&[
            (HOST, "127.0.0.1:5000"),
            (ORIGIN, "https://evil.example.com"),
            (AUTHORIZATION, "Bearer abc"),
        ]);
        assert!(matches!(
            check_request(&map),
            Err(AuthRejection::Forbidden(_))
        ));
    }

    #[test]
    fn rejects_missing_and_malformed_tokens() {
        let map = headers(&[(HOST, "127.0.0.1:5000")]);
        assert!(matches!(
            check_request(&map),
            Err(AuthRejection::Unauthorized(_))
        ));

        let map = headers(&[(HOST, "127.0.0.1:5000"), (AUTHORIZATION, "Basic abc")]);
        assert!(matches!(
            check_request(&map),
            Err(AuthRejection::Unauthorized(_))
        ));
    }
}
