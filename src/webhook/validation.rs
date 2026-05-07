use std::net::IpAddr;

use crate::tus::TusError;

pub async fn validate_webhook_url(url: &str) -> Result<(), TusError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| TusError::InvalidHeader(format!("webhook URL is invalid: {url}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(TusError::InvalidHeader(format!(
                "webhook URL scheme must be http or https, got: {scheme}"
            )))
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| TusError::InvalidHeader("webhook URL has no host".into()))?;

    let port = parsed.port_or_known_default().unwrap_or(80);

    let addrs = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| {
            TusError::InvalidHeader(format!("webhook URL host could not be resolved: {e}"))
        })?;

    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            return Err(TusError::InvalidHeader(format!(
                "webhook URL resolves to a blocked address: {}",
                addr.ip()
            )));
        }
    }

    Ok(())
}

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()       // 127.0.0.0/8
                || v4.is_private() // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254.0.0/16 (incl. AWS metadata)
                || v4.is_unspecified() // 0.0.0.0
                || v4.is_broadcast()   // 255.255.255.255
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()        // ::1
                || v6.is_unspecified() // ::
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique-local (ULA)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use super::is_blocked_ip;

    #[test]
    fn blocks_loopback_v4() {
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    }

    #[test]
    fn blocks_private_ranges() {
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn blocks_link_local_and_metadata() {
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1))));
    }

    #[test]
    fn blocks_ipv6_loopback_and_ula() {
        assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_blocked_ip("fe80::1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip("fc00::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn allows_public_ips() {
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_blocked_ip("2606:4700:4700::1111".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn rejects_non_http_scheme() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(super::validate_webhook_url("ftp://example.com/hook").await.is_err());
            assert!(super::validate_webhook_url("file:///etc/passwd").await.is_err());
            assert!(super::validate_webhook_url("not a url").await.is_err());
        });
    }
}
