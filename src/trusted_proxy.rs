use std::net::IpAddr;

use axum::http::HeaderMap;
use ipnet::IpNet;

/// Extract the real client IP from request headers.
///
/// Forwarded headers (`X-Forwarded-For`, `X-Real-IP`) are trusted only when the TCP peer
/// address falls within one of the `trusted` CIDRs. When `trusted` is empty (the default),
/// forwarded headers are ignored and the TCP peer is used directly — this prevents IP
/// spoofing on internet-facing deployments that have no configured proxy.
///
/// To honour forwarded headers, set `TRUSTED_PROXIES` to the CIDRs of your reverse proxy
/// (e.g. `10.0.0.0/8,172.16.0.0/12,192.168.0.0/16` for a typical Docker network).
pub fn extract_client_ip(headers: &HeaderMap, peer: IpAddr, trusted: &[IpNet]) -> IpAddr {
    if trusted.is_empty() || !trusted.iter().any(|net| net.contains(&peer)) {
        return peer;
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(peer)
}
