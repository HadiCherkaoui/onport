// Rust guideline compliant 2026-02-16

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{bail, Context, Result};

use crate::types::{PortEntry, Protocol, SocketState};

use super::PlatformProvider;

/// FreeBSD socket provider — enumerates sockets via `sockstat`.
pub struct FreeBsdProvider;

impl PlatformProvider for FreeBsdProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        let output = run_sockstat()?;
        Ok(parse_sockstat_output(&output))
    }
}

/// Run `sockstat` and return its stdout.
///
/// # Errors
///
/// Returns an error if `sockstat` is not found or exits with an
/// unexpected failure code.
fn run_sockstat() -> Result<String> {
    let output = std::process::Command::new("sockstat")
        .args(["-s", "-P", "tcp,udp", "-q", "-n"])
        .output()
        .context("Failed to run sockstat")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "sockstat failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse a sockstat address field (`host:port`) into an IP and port.
///
/// FreeBSD sockstat shows IPv6 addresses without brackets (e.g., `::1:80`),
/// so we always split on the **last** `:` and use `is_ipv6` from the PROTO
/// field to determine address family. Scope IDs (`%lo0`) are stripped.
///
/// Returns `None` for unparseable addresses or `*:*` (unbound sockets).
fn parse_sockstat_address(s: &str, is_ipv6: bool) -> Option<(IpAddr, u16)> {
    if let Some(port_str) = s.strip_prefix("*:") {
        let port = port_str.parse::<u16>().ok()?;
        let addr = if is_ipv6 {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        };
        return Some((addr, port));
    }

    let colon_pos = s.rfind(':')?;
    let host = &s[..colon_pos];
    let port_str = &s[colon_pos + 1..];
    let port = port_str.parse::<u16>().ok()?;

    let addr = if is_ipv6 {
        // Strip scope ID if present (e.g., "fe80::1%lo0" -> "fe80::1").
        let clean = match host.find('%') {
            Some(pos) => &host[..pos],
            None => host,
        };
        let ipv6: Ipv6Addr = clean.parse().ok()?;
        IpAddr::V6(ipv6)
    } else {
        let ipv4: Ipv4Addr = host.parse().ok()?;
        IpAddr::V4(ipv4)
    };

    Some((addr, port))
}

/// Map a sockstat TCP state string to `SocketState`.
///
/// FreeBSD sockstat reports standard TCP state names. Mappings match
/// `parse_lsof_state` (macOS) and `SocketState::from_win_state` (Windows)
/// for cross-platform parity.
fn parse_sockstat_state(state: &str) -> SocketState {
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

/// Parse a PROTO field into protocol and address-family flag.
///
/// Returns `(Protocol, is_ipv6)` or `None` for unsupported protocols.
fn parse_proto(proto: &str) -> Option<(Protocol, bool)> {
    match proto {
        "tcp4" => Some((Protocol::Tcp, false)),
        "tcp6" | "tcp46" => Some((Protocol::Tcp, true)),
        "udp4" => Some((Protocol::Udp, false)),
        "udp6" | "udp46" => Some((Protocol::Udp, true)),
        _ => None,
    }
}

/// Parse the full output of `sockstat -s -P tcp,udp -q -n`.
///
/// Each line is one socket. Fields are whitespace-delimited:
/// `USER COMMAND PID FD PROTO LOCAL FOREIGN [STATE]`
/// TCP lines have 8 fields (with STATE), UDP lines have 7.
/// Malformed lines are silently skipped per the graceful-degradation principle.
fn parse_sockstat_output(output: &str) -> Vec<PortEntry> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 7 {
                return None;
            }

            let user = fields[0];
            let command = fields[1];
            let pid: u32 = fields[2].parse().ok()?;
            // fields[3] is FD — ignored
            let (protocol, is_ipv6) = parse_proto(fields[4])?;

            let (local_addr, port) = parse_sockstat_address(fields[5], is_ipv6)?;

            // Foreign address: *:* means no remote connection.
            let remote_addr = parse_sockstat_address(fields[6], is_ipv6)
                .map(|(addr, rport)| SocketAddr::new(addr, rport));

            // TCP lines have a state column (index 7); UDP sockets are forced to Listen.
            let state = if protocol == Protocol::Udp {
                SocketState::Listen
            } else if let Some(&state_str) = fields.get(7) {
                parse_sockstat_state(state_str)
            } else {
                SocketState::Other("UNKNOWN".to_string())
            };

            Some(PortEntry {
                port,
                protocol,
                state,
                pid: Some(pid),
                process_name: Some(command.to_string()),
                user: Some(user.to_string()),
                local_addr,
                remote_addr,
                docker_container: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_sockstat_address ────────────────────────────────────────────────

    #[test]
    fn test_parse_addr_wildcard_ipv4() {
        let (addr, port) = parse_sockstat_address("*:80", false).unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_addr_wildcard_ipv6() {
        let (addr, port) = parse_sockstat_address("*:22", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(port, 22);
    }

    #[test]
    fn test_parse_addr_ipv4_specific() {
        let (addr, port) = parse_sockstat_address("192.168.1.87:443", false).unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 87)));
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_addr_ipv6_localhost() {
        // FreeBSD sockstat: "::1:8080" — no brackets, last colon separates port.
        let (addr, port) = parse_sockstat_address("::1:8080", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_addr_ipv6_unspecified() {
        // FreeBSD sockstat: ":::443" — [::] with port 443.
        let (addr, port) = parse_sockstat_address(":::443", true).unwrap();
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_addr_ipv6_full() {
        let (addr, port) = parse_sockstat_address("fe80::1:80", true).unwrap();
        assert_eq!(addr, IpAddr::V6("fe80::1".parse::<Ipv6Addr>().unwrap()));
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_addr_ipv6_with_scope_id() {
        // sockstat may include scope IDs like "fe80::1%lo0:80".
        let (addr, port) = parse_sockstat_address("fe80::1%lo0:80", true).unwrap();
        assert_eq!(addr, IpAddr::V6("fe80::1".parse::<Ipv6Addr>().unwrap()));
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_addr_star_star_returns_none() {
        assert!(parse_sockstat_address("*:*", false).is_none());
    }

    #[test]
    fn test_parse_addr_empty_returns_none() {
        assert!(parse_sockstat_address("", false).is_none());
    }

    #[test]
    fn test_parse_addr_high_port() {
        let (_, port) = parse_sockstat_address("0.0.0.0:65535", false).unwrap();
        assert_eq!(port, 65535);
    }

    // ── parse_sockstat_state ──────────────────────────────────────────────────

    #[test]
    fn test_parse_state_listen() {
        assert_eq!(parse_sockstat_state("LISTEN"), SocketState::Listen);
    }

    #[test]
    fn test_parse_state_established() {
        assert_eq!(parse_sockstat_state("ESTABLISHED"), SocketState::Established);
    }

    #[test]
    fn test_parse_state_time_wait() {
        assert_eq!(parse_sockstat_state("TIME_WAIT"), SocketState::TimeWait);
    }

    #[test]
    fn test_parse_state_close_wait() {
        assert_eq!(parse_sockstat_state("CLOSE_WAIT"), SocketState::CloseWait);
    }

    #[test]
    fn test_parse_state_syn_sent() {
        assert_eq!(parse_sockstat_state("SYN_SENT"), SocketState::SynSent);
    }

    #[test]
    fn test_parse_state_syn_recv() {
        assert_eq!(parse_sockstat_state("SYN_RECEIVED"), SocketState::SynRecv);
    }

    #[test]
    fn test_parse_state_closed() {
        assert_eq!(parse_sockstat_state("CLOSED"), SocketState::Close);
    }

    #[test]
    fn test_parse_state_fin_wait() {
        assert_eq!(
            parse_sockstat_state("FIN_WAIT_1"),
            SocketState::Other("FIN_WAIT".to_string())
        );
        assert_eq!(
            parse_sockstat_state("FIN_WAIT_2"),
            SocketState::Other("FIN_WAIT".to_string())
        );
    }

    #[test]
    fn test_parse_state_closing_and_last_ack() {
        assert_eq!(
            parse_sockstat_state("CLOSING"),
            SocketState::Other("CLOSING".to_string())
        );
        assert_eq!(
            parse_sockstat_state("LAST_ACK"),
            SocketState::Other("CLOSING".to_string())
        );
    }

    #[test]
    fn test_parse_state_unknown() {
        assert_eq!(
            parse_sockstat_state("WEIRD"),
            SocketState::Other("WEIRD".to_string())
        );
    }

    // ── parse_sockstat_output ─────────────────────────────────────────────────

    const SAMPLE_SOCKSTAT_OUTPUT: &str = "\
root     sshd       781   4  tcp4   *:22                  *:*                   LISTEN
www      nginx      97042 8  tcp4   *:443                 *:*                   LISTEN
www      nginx      97042 9  tcp4   192.168.1.87:443      192.168.1.64:60910    ESTABLISHED
root     sshd       781   5  tcp6   *:22                  *:*                   LISTEN
root     ntpd       856   20 udp4   *:123                 *:*
";

    #[test]
    fn test_parse_sockstat_output_full() {
        let entries = parse_sockstat_output(SAMPLE_SOCKSTAT_OUTPUT);
        assert_eq!(entries.len(), 5);

        // sshd listening on *:22 (IPv4)
        assert_eq!(entries[0].pid, Some(781));
        assert_eq!(entries[0].process_name, Some("sshd".to_string()));
        assert_eq!(entries[0].user, Some("root".to_string()));
        assert_eq!(entries[0].port, 22);
        assert_eq!(entries[0].protocol, Protocol::Tcp);
        assert_eq!(entries[0].state, SocketState::Listen);
        assert_eq!(entries[0].local_addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert!(entries[0].remote_addr.is_none());

        // nginx listening on *:443
        assert_eq!(entries[1].pid, Some(97042));
        assert_eq!(entries[1].process_name, Some("nginx".to_string()));
        assert_eq!(entries[1].port, 443);

        // nginx established connection
        assert_eq!(entries[2].state, SocketState::Established);
        let remote = entries[2].remote_addr.unwrap();
        assert_eq!(remote.ip(), IpAddr::V4(Ipv4Addr::new(192, 168, 1, 64)));
        assert_eq!(remote.port(), 60910);

        // sshd listening on *:22 (IPv6)
        assert_eq!(entries[3].local_addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(entries[3].state, SocketState::Listen);

        // ntpd UDP *:123 — forced to SocketState::Listen
        assert_eq!(entries[4].protocol, Protocol::Udp);
        assert_eq!(entries[4].state, SocketState::Listen);
        assert_eq!(entries[4].port, 123);
    }

    #[test]
    fn test_parse_sockstat_output_ipv6_addresses() {
        let output =
            "root     sshd       781   6  tcp6   :::443                :::22                 ESTABLISHED\n";
        let entries = parse_sockstat_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].local_addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(entries[0].port, 443);
        let remote = entries[0].remote_addr.unwrap();
        assert_eq!(remote.ip(), IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(remote.port(), 22);
    }

    #[test]
    fn test_parse_sockstat_output_empty() {
        assert!(parse_sockstat_output("").is_empty());
    }

    #[test]
    fn test_parse_sockstat_output_skips_malformed_line() {
        let output = "this is not a valid sockstat line\n";
        assert!(parse_sockstat_output(output).is_empty());
    }

    #[test]
    fn test_parse_sockstat_output_skips_unknown_proto() {
        // Unix domain sockets have proto "stream" — skip them.
        let output = "root     syslogd    427   6  stream /var/run/log\n";
        assert!(parse_sockstat_output(output).is_empty());
    }

    #[test]
    fn test_parse_sockstat_output_remote_star_star_becomes_none() {
        let output =
            "root     sshd       781   4  tcp4   *:22                  *:*                   LISTEN\n";
        let entries = parse_sockstat_output(output);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].remote_addr.is_none());
    }
}
