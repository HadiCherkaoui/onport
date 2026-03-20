// Rust guideline compliant 2026-02-16

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{bail, Context, Result};

use crate::types::{PortEntry, Protocol, SocketState};

use super::PlatformProvider;

/// macOS socket provider — enumerates sockets via `lsof -F`.
pub struct MacOsProvider;

impl PlatformProvider for MacOsProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        bail!("macOS support not yet implemented")
    }
}

/// Parse a single `host:port` component from lsof output.
///
/// Supported formats:
/// - `*:port` — unspecified address (IPv4 or IPv6 based on `is_ipv6`)
/// - `[ipv6]:port` — IPv6 address in brackets (scope IDs stripped)
/// - `ipv4:port` — IPv4 address (split on last `:`)
fn parse_host_port(s: &str, is_ipv6: bool) -> Option<(IpAddr, u16)> {
    if let Some(port_str) = s.strip_prefix("*:") {
        let port = port_str.parse::<u16>().ok()?;
        let addr = if is_ipv6 {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        };
        return Some((addr, port));
    }

    if s.starts_with('[') {
        let close_bracket = s.find(']')?;
        let ipv6_str = &s[1..close_bracket];
        // Strip scope ID if present (e.g., "fe80::1%lo0" -> "fe80::1").
        let ipv6_clean = match ipv6_str.find('%') {
            Some(pos) => &ipv6_str[..pos],
            None => ipv6_str,
        };
        let addr: Ipv6Addr = ipv6_clean.parse().ok()?;
        // Skip past "]:" to reach the port string.
        let port_str = s.get(close_bracket + 2..)?;
        let port = port_str.parse::<u16>().ok()?;
        return Some((IpAddr::V6(addr), port));
    }

    // IPv4: split on the last ':' to handle the address correctly.
    let colon_pos = s.rfind(':')?;
    let host = &s[..colon_pos];
    let port_str = &s[colon_pos + 1..];
    let addr: Ipv4Addr = host.parse().ok()?;
    let port = port_str.parse::<u16>().ok()?;
    Some((IpAddr::V4(addr), port))
}

/// Parse the lsof `n` (name) field into address components.
///
/// `is_ipv6` comes from the `t` (file type) field and is needed to
/// disambiguate `*:port` (IPv4 vs IPv6 wildcard). Connected sockets
/// use `->` to separate local and remote parts.
///
/// Returns `None` for non-socket names (e.g., `*:*`, garbage strings).
fn parse_name_field(name: &str, is_ipv6: bool) -> Option<(IpAddr, u16, Option<SocketAddr>)> {
    let (local_part, remote_part) = match name.split_once("->") {
        Some((l, r)) => (l, Some(r)),
        None => (name, None),
    };

    let (local_addr, local_port) = parse_host_port(local_part, is_ipv6)?;
    let remote = remote_part.and_then(|r| {
        let (addr, port) = parse_host_port(r, is_ipv6)?;
        Some(SocketAddr::new(addr, port))
    });

    Some((local_addr, local_port, remote))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_name_field ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_name_wildcard_ipv4() {
        let (addr, port, remote) = parse_name_field("*:80", false).unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(port, 80);
        assert!(remote.is_none());
    }

    #[test]
    fn test_parse_name_wildcard_ipv6() {
        // When tIPv6 is reported, *:port means [::] not 0.0.0.0.
        let (addr, port, remote) = parse_name_field("*:80", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(port, 80);
        assert!(remote.is_none());
    }

    #[test]
    fn test_parse_name_ipv4_listen() {
        let (addr, port, remote) = parse_name_field("127.0.0.1:8080", false).unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(port, 8080);
        assert!(remote.is_none());
    }

    #[test]
    fn test_parse_name_ipv6_listen() {
        let (addr, port, remote) = parse_name_field("[::1]:8080", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(port, 8080);
        assert!(remote.is_none());
    }

    #[test]
    fn test_parse_name_ipv6_wildcard_brackets() {
        let (addr, port, remote) = parse_name_field("[::]:80", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(port, 80);
        assert!(remote.is_none());
    }

    #[test]
    fn test_parse_name_ipv4_connected() {
        let (addr, port, remote) =
            parse_name_field("127.0.0.1:8080->10.0.0.1:443", false).unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(port, 8080);
        let r = remote.unwrap();
        assert_eq!(r.ip(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(r.port(), 443);
    }

    #[test]
    fn test_parse_name_ipv6_connected() {
        let (addr, port, remote) = parse_name_field("[::1]:8080->[::1]:443", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(port, 8080);
        let r = remote.unwrap();
        assert_eq!(r.ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(r.port(), 443);
    }

    #[test]
    fn test_parse_name_ipv6_with_scope_id() {
        let (addr, port, _) = parse_name_field("[fe80::1%lo0]:8080", true).unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V6("fe80::1".parse::<Ipv6Addr>().unwrap()));
    }

    #[test]
    fn test_parse_name_garbage_returns_none() {
        assert!(parse_name_field("not-a-socket", false).is_none());
        assert!(parse_name_field("", false).is_none());
    }

    #[test]
    fn test_parse_name_star_star_returns_none() {
        // Unbound UDP sockets may show *:* — not a usable port.
        assert!(parse_name_field("*:*", false).is_none());
    }

    #[test]
    fn test_parse_name_high_port() {
        let (_, port, _) = parse_name_field("0.0.0.0:65535", false).unwrap();
        assert_eq!(port, 65535);
    }
}
