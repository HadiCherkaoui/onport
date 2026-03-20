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

/// Map an lsof TCP state string to `SocketState`.
///
/// macOS lsof reports standard TCP state names. Mappings are kept
/// consistent with `SocketState::from_win_state` for cross-platform
/// parity (e.g., both `FIN_WAIT_1` and `FIN_WAIT_2` become
/// `Other("FIN_WAIT")`).
fn parse_lsof_state(state: &str) -> SocketState {
    match state {
        "LISTEN" => SocketState::Listen,
        "ESTABLISHED" => SocketState::Established,
        "TIME_WAIT" => SocketState::TimeWait,
        "CLOSE_WAIT" => SocketState::CloseWait,
        "SYN_SENT" => SocketState::SynSent,
        "SYN_RECEIVED" | "SYN_RECV" => SocketState::SynRecv,
        "CLOSED" => SocketState::Close,
        "FIN_WAIT_1" | "FIN_WAIT_2" => SocketState::Other("FIN_WAIT".to_string()),
        "CLOSING" | "LAST_ACK" => SocketState::Other("CLOSING".to_string()),
        other => SocketState::Other(other.to_string()),
    }
}

/// Intermediate context for the current process being parsed.
struct ProcessContext {
    pid: u32,
    command: Option<String>,
    user: Option<String>,
}

/// Intermediate context for the current file descriptor being parsed.
struct FdContext {
    protocol: Option<Protocol>,
    state: Option<SocketState>,
    is_ipv6: bool,
}

/// Parse the full output of `lsof -iTCP -iUDP -nP -F pcLtPTn`.
///
/// Walks each line using a two-level state machine (process context +
/// FD context). Emits a `PortEntry` on every `n` (name) line that
/// has both a valid protocol and parseable address. Malformed lines
/// are silently skipped per the graceful-degradation project principle.
fn parse_lsof_output(output: &str) -> Vec<PortEntry> {
    let mut entries = Vec::new();
    let mut process: Option<ProcessContext> = None;
    let mut fd = FdContext {
        protocol: None,
        state: None,
        is_ipv6: false,
    };

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let (field_id, value) = line.split_at(1);
        let field = match field_id.as_bytes().first() {
            Some(&b) => b,
            None => continue,
        };

        match field {
            b'p' => {
                let Ok(pid) = value.parse::<u32>() else {
                    continue;
                };
                process = Some(ProcessContext {
                    pid,
                    command: None,
                    user: None,
                });
                fd = FdContext {
                    protocol: None,
                    state: None,
                    is_ipv6: false,
                };
            }
            b'c' => {
                if let Some(ref mut proc) = process {
                    proc.command = Some(value.to_string());
                }
            }
            b'L' => {
                if let Some(ref mut proc) = process {
                    proc.user = Some(value.to_string());
                }
            }
            b'f' => {
                fd = FdContext {
                    protocol: None,
                    state: None,
                    is_ipv6: false,
                };
            }
            b't' => {
                fd.is_ipv6 = value == "IPv6";
            }
            b'P' => {
                fd.protocol = match value {
                    "TCP" => Some(Protocol::Tcp),
                    "UDP" => Some(Protocol::Udp),
                    _ => None,
                };
            }
            b'T' => {
                if let Some(state_str) = value.strip_prefix("ST=") {
                    fd.state = Some(parse_lsof_state(state_str));
                }
            }
            b'n' => {
                let Some(ref proc) = process else {
                    continue;
                };
                let Some(ref protocol) = fd.protocol else {
                    continue;
                };
                let Some((local_addr, port, remote_addr)) =
                    parse_name_field(value, fd.is_ipv6)
                else {
                    continue;
                };

                // UDP sockets have no meaningful TCP state; treat as Listen.
                let state = if *protocol == Protocol::Udp {
                    SocketState::Listen
                } else {
                    fd.state
                        .clone()
                        .unwrap_or_else(|| SocketState::Other("UNKNOWN".to_string()))
                };

                entries.push(PortEntry {
                    port,
                    protocol: protocol.clone(),
                    state,
                    pid: Some(proc.pid),
                    process_name: proc.command.clone(),
                    user: proc.user.clone(),
                    local_addr,
                    remote_addr,
                    docker_container: None,
                });
            }
            _ => {}
        }
    }

    entries
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

    // ── parse_lsof_state ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_lsof_state_listen() {
        assert_eq!(parse_lsof_state("LISTEN"), SocketState::Listen);
    }

    #[test]
    fn test_parse_lsof_state_established() {
        assert_eq!(parse_lsof_state("ESTABLISHED"), SocketState::Established);
    }

    #[test]
    fn test_parse_lsof_state_time_wait() {
        assert_eq!(parse_lsof_state("TIME_WAIT"), SocketState::TimeWait);
    }

    #[test]
    fn test_parse_lsof_state_close_wait() {
        assert_eq!(parse_lsof_state("CLOSE_WAIT"), SocketState::CloseWait);
    }

    #[test]
    fn test_parse_lsof_state_syn_sent() {
        assert_eq!(parse_lsof_state("SYN_SENT"), SocketState::SynSent);
    }

    #[test]
    fn test_parse_lsof_state_syn_recv() {
        assert_eq!(parse_lsof_state("SYN_RECEIVED"), SocketState::SynRecv);
    }

    #[test]
    fn test_parse_lsof_state_closed() {
        assert_eq!(parse_lsof_state("CLOSED"), SocketState::Close);
    }

    #[test]
    fn test_parse_lsof_state_fin_wait() {
        // FIN_WAIT states map to Other, consistent with Windows behavior.
        assert_eq!(
            parse_lsof_state("FIN_WAIT_1"),
            SocketState::Other("FIN_WAIT".to_string())
        );
        assert_eq!(
            parse_lsof_state("FIN_WAIT_2"),
            SocketState::Other("FIN_WAIT".to_string())
        );
    }

    #[test]
    fn test_parse_lsof_state_closing_and_last_ack() {
        // Consistent with Windows: CLOSING and LAST_ACK map to Other("CLOSING").
        assert_eq!(
            parse_lsof_state("CLOSING"),
            SocketState::Other("CLOSING".to_string())
        );
        assert_eq!(
            parse_lsof_state("LAST_ACK"),
            SocketState::Other("CLOSING".to_string())
        );
    }

    #[test]
    fn test_parse_lsof_state_unknown() {
        assert_eq!(
            parse_lsof_state("WEIRD"),
            SocketState::Other("WEIRD".to_string())
        );
    }

    // ── parse_lsof_output ────────────────────────────────────────────────────

    const SAMPLE_LSOF_OUTPUT: &str = "\
p1234
cnginx
Lroot
f6
tIPv4
PTCP
TST=LISTEN
n*:80
f7
tIPv6
PTCP
TST=LISTEN
n[::]:80
f8
tIPv4
PTCP
TST=ESTABLISHED
n192.168.1.1:80->10.0.0.5:52341
p5678
cnode
Lhch
f12
tIPv4
PTCP
TST=LISTEN
n127.0.0.1:3000
f15
tIPv4
PUDP
n*:5353
";

    #[test]
    fn test_parse_lsof_output_full() {
        let entries = parse_lsof_output(SAMPLE_LSOF_OUTPUT);
        assert_eq!(entries.len(), 5);

        // nginx listening on *:80 (IPv4)
        assert_eq!(entries[0].pid, Some(1234));
        assert_eq!(entries[0].process_name, Some("nginx".to_string()));
        assert_eq!(entries[0].user, Some("root".to_string()));
        assert_eq!(entries[0].port, 80);
        assert_eq!(entries[0].protocol, Protocol::Tcp);
        assert_eq!(entries[0].state, SocketState::Listen);
        assert_eq!(entries[0].local_addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert!(entries[0].remote_addr.is_none());

        // nginx listening on [::]:80 (IPv6)
        assert_eq!(entries[1].pid, Some(1234));
        assert_eq!(entries[1].local_addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));

        // nginx established connection
        assert_eq!(entries[2].state, SocketState::Established);
        let remote = entries[2].remote_addr.unwrap();
        assert_eq!(remote.port(), 52341);

        // node on 127.0.0.1:3000
        assert_eq!(entries[3].pid, Some(5678));
        assert_eq!(entries[3].process_name, Some("node".to_string()));
        assert_eq!(entries[3].user, Some("hch".to_string()));
        assert_eq!(entries[3].port, 3000);

        // node UDP *:5353 — forced to SocketState::Listen
        assert_eq!(entries[4].protocol, Protocol::Udp);
        assert_eq!(entries[4].state, SocketState::Listen);
        assert_eq!(entries[4].port, 5353);
    }

    #[test]
    fn test_parse_lsof_output_ipv6_wildcard_via_t_field() {
        // When tIPv6 is set and name is *:port, local_addr must be [::].
        let output = "p100\nctest\nLuser\nf1\ntIPv6\nPTCP\nTST=LISTEN\nn*:443\n";
        let entries = parse_lsof_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].local_addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(entries[0].port, 443);
    }

    #[test]
    fn test_parse_lsof_output_empty() {
        assert!(parse_lsof_output("").is_empty());
    }

    #[test]
    fn test_parse_lsof_output_process_with_no_sockets() {
        let output = "p999\nclaunchd\nLroot\n";
        assert!(parse_lsof_output(output).is_empty());
    }

    #[test]
    fn test_parse_lsof_output_skips_malformed_name() {
        let output = "p100\ncbad\nLuser\nf1\ntIPv4\nPTCP\nTST=LISTEN\nnot-a-valid-address\n";
        assert!(parse_lsof_output(output).is_empty());
    }

    #[test]
    fn test_parse_lsof_output_missing_protocol_skips_fd() {
        let output = "p100\nctest\nLuser\nf1\ntIPv4\nTST=LISTEN\nn*:80\n";
        assert!(parse_lsof_output(output).is_empty());
    }
}
