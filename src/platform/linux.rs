// Rust guideline compliant 2026-03-20

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::Result;

use crate::types::{PortEntry, Protocol, SocketState};

use super::PlatformProvider;

/// Linux socket provider reading `/proc/net/{tcp,tcp6,udp,udp6}`.
///
/// Reads all four `/proc/net/` socket files to enumerate sockets,
/// then resolves PIDs by scanning `/proc/{pid}/fd/` for socket inodes.
pub struct LinuxProvider;

/// Intermediate parsed socket before PID resolution.
struct RawSocketEntry {
    port: u16,
    local_addr: IpAddr,
    remote_addr: Option<SocketAddr>,
    state: SocketState,
    uid: u32,
    inode: u64,
    protocol: Protocol,
}

impl PlatformProvider for LinuxProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        let mut raw_entries = Vec::new();
        raw_entries.extend(read_proc_net("/proc/net/tcp", false, Protocol::Tcp));
        raw_entries.extend(read_proc_net("/proc/net/tcp6", true, Protocol::Tcp));
        raw_entries.extend(read_proc_net("/proc/net/udp", false, Protocol::Udp));
        raw_entries.extend(read_proc_net("/proc/net/udp6", true, Protocol::Udp));

        let inode_to_pid = build_inode_to_pid_map();
        let username_cache = build_username_cache();

        let entries = raw_entries
            .into_iter()
            .map(|raw| {
                let pid = inode_to_pid.get(&raw.inode).copied();
                let process_name = pid.and_then(get_process_name);
                let user = username_cache
                    .get(&raw.uid)
                    .cloned()
                    .or_else(|| Some(raw.uid.to_string()));

                // UDP sockets don't have TCP state machine; treat all as LISTEN.
                let state = if raw.protocol == Protocol::Udp {
                    SocketState::Listen
                } else {
                    raw.state
                };

                PortEntry {
                    port: raw.port,
                    protocol: raw.protocol,
                    state,
                    pid,
                    process_name,
                    user,
                    local_addr: raw.local_addr,
                    remote_addr: raw.remote_addr,
                    docker_container: None,
                }
            })
            .collect();

        Ok(entries)
    }
}

/// Parse an IPv4 address from little-endian hex.
///
/// `/proc/net/tcp` stores IPv4 in native (little-endian on x86) byte order.
fn parse_hex_ipv4(hex: &str) -> Option<Ipv4Addr> {
    let val = u32::from_str_radix(hex, 16).ok()?;
    Some(Ipv4Addr::from(val.to_be()))
}

/// Parse an IPv6 address from hex groups.
///
/// `/proc/net/tcp6` stores IPv6 as four 32-bit words in host byte order.
fn parse_hex_ipv6(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let mut octets = [0u8; 16];
    for i in 0..4 {
        let word_hex = &hex[i * 8..(i + 1) * 8];
        let word = u32::from_str_radix(word_hex, 16).ok()?;
        let bytes = word.to_be_bytes();
        octets[i * 4] = bytes[3];
        octets[i * 4 + 1] = bytes[2];
        octets[i * 4 + 2] = bytes[1];
        octets[i * 4 + 3] = bytes[0];
    }
    Some(Ipv6Addr::from(octets))
}

/// Parse a hex port string to u16.
fn parse_hex_port(hex: &str) -> Option<u16> {
    u16::from_str_radix(hex, 16).ok()
}

/// Parse a single line from `/proc/net/tcp`, `/proc/net/tcp6`, `/proc/net/udp`, or `/proc/net/udp6`.
///
/// Returns `None` for header lines or unparseable entries.
fn parse_proc_line(line: &str, is_ipv6: bool, protocol: Protocol) -> Option<RawSocketEntry> {
    let line = line.trim();
    if line.starts_with("sl") || line.is_empty() {
        return None;
    }

    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }

    // Field 1: local_address (hex_ip:hex_port)
    let local_parts: Vec<&str> = fields[1].split(':').collect();
    if local_parts.len() != 2 {
        return None;
    }

    let local_addr: IpAddr = if is_ipv6 {
        IpAddr::V6(parse_hex_ipv6(local_parts[0])?)
    } else {
        IpAddr::V4(parse_hex_ipv4(local_parts[0])?)
    };
    let port = parse_hex_port(local_parts[1])?;

    // Field 2: rem_address (hex_ip:hex_port); zero port means no remote peer.
    let remote_parts: Vec<&str> = fields[2].split(':').collect();
    let remote_addr = if remote_parts.len() == 2 {
        let remote_port = parse_hex_port(remote_parts[1]).unwrap_or(0);
        if remote_port == 0 {
            None
        } else if is_ipv6 {
            parse_hex_ipv6(remote_parts[0])
                .map(|ip| SocketAddr::new(IpAddr::V6(ip), remote_port))
        } else {
            parse_hex_ipv4(remote_parts[0])
                .map(|ip| SocketAddr::new(IpAddr::V4(ip), remote_port))
        }
    } else {
        None
    };

    // Field 3: state (for UDP, kernel reports 07 for bound sockets)
    let state = SocketState::from_hex(fields[3]);

    // Field 7: uid
    let uid: u32 = fields[7].parse().ok()?;

    // Field 9: inode
    let inode: u64 = fields[9].parse().ok()?;

    Some(RawSocketEntry {
        port,
        local_addr,
        remote_addr,
        state,
        uid,
        inode,
        protocol,
    })
}

/// Read and parse a `/proc/net/` file.
fn read_proc_net(path: &str, is_ipv6: bool, protocol: Protocol) -> Vec<RawSocketEntry> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter_map(|line| parse_proc_line(line, is_ipv6, protocol.clone()))
        .collect()
}

/// Build a map from socket inode to PID by scanning `/proc/{pid}/fd/`.
///
/// Silently skips processes we lack permission to read.
fn build_inode_to_pid_map() -> HashMap<u64, u32> {
    let mut map = HashMap::new();

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return map,
    };

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let fd_path = format!("/proc/{pid}/fd");
        let fd_dir = match fs::read_dir(&fd_path) {
            Ok(d) => d,
            Err(_) => continue, // Permission denied or process exited
        };

        for fd_entry in fd_dir.flatten() {
            let link = match fs::read_link(fd_entry.path()) {
                Ok(l) => l,
                Err(_) => continue,
            };

            let link_str = link.to_string_lossy();
            if let Some(inode_str) = link_str
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
            {
                if let Ok(inode) = inode_str.parse::<u64>() {
                    map.insert(inode, pid);
                }
            }
        }
    }

    map
}

/// Read the process name from `/proc/{pid}/comm`.
fn get_process_name(pid: u32) -> Option<String> {
    let path = format!("/proc/{pid}/comm");
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Build a cache mapping UID to username from `/etc/passwd`.
fn build_username_cache() -> HashMap<u32, String> {
    let mut cache = HashMap::new();

    let content = match fs::read_to_string("/etc/passwd") {
        Ok(c) => c,
        Err(_) => return cache,
    };

    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 {
            if let Ok(uid) = fields[2].parse::<u32>() {
                cache.insert(uid, fields[0].to_string());
            }
        }
    }

    cache
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    // Sample line from /proc/net/tcp (127.0.0.1:8080, LISTEN, uid=1000, inode=12345)
    const SAMPLE_TCP_LINE: &str =
        "   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1 0000000000000000 100 0 0 10 0";

    // Established connection: 127.0.0.1:8080 -> 10.0.0.5:443
    const SAMPLE_TCP_ESTABLISHED: &str =
        "   1: 0100007F:1F90 0500000A:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 23456 1 0000000000000000 100 0 0 10 0";

    // Sample line from /proc/net/udp (0.0.0.0:53, state=07 = bound socket, uid=101, inode=67890)
    const SAMPLE_UDP_LINE: &str =
        "   0: 00000000:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000   101        0 67890 2 0000000000000000";

    #[test]
    fn test_parse_proc_net_tcp_line() {
        let entry = parse_proc_line(SAMPLE_TCP_LINE, false, Protocol::Tcp).expect("should parse");
        assert_eq!(entry.local_addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.state, SocketState::Listen);
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.inode, 12345);
        assert_eq!(entry.protocol, Protocol::Tcp);
    }

    #[test]
    fn test_parse_proc_net_udp_line() {
        let entry = parse_proc_line(SAMPLE_UDP_LINE, false, Protocol::Udp).expect("should parse");
        assert_eq!(entry.local_addr, IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        assert_eq!(entry.port, 53);
        assert_eq!(entry.state, SocketState::Close); // raw state from /proc/net/udp
        assert_eq!(entry.uid, 101);
        assert_eq!(entry.inode, 67890);
        assert_eq!(entry.protocol, Protocol::Udp);
    }

    #[test]
    fn test_parse_hex_ipv4() {
        let addr = parse_hex_ipv4("0100007F").expect("should parse");
        assert_eq!(addr, Ipv4Addr::new(127, 0, 0, 1));
    }

    #[test]
    fn test_parse_hex_ipv4_any() {
        let addr = parse_hex_ipv4("00000000").expect("should parse");
        assert_eq!(addr, Ipv4Addr::new(0, 0, 0, 0));
    }

    #[test]
    fn test_parse_hex_port() {
        assert_eq!(parse_hex_port("1F90"), Some(8080));
        assert_eq!(parse_hex_port("0050"), Some(80));
        assert_eq!(parse_hex_port("0016"), Some(22));
    }

    #[test]
    fn test_parse_proc_net_tcp_established_has_remote() {
        let entry =
            parse_proc_line(SAMPLE_TCP_ESTABLISHED, false, Protocol::Tcp).expect("should parse");
        assert_eq!(entry.local_addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.state, SocketState::Established);
        let remote = entry
            .remote_addr
            .expect("established connection should have remote_addr");
        assert_eq!(remote.ip(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(remote.port(), 443);
    }

    #[test]
    fn test_parse_proc_net_tcp_listen_has_no_remote() {
        let entry =
            parse_proc_line(SAMPLE_TCP_LINE, false, Protocol::Tcp).expect("should parse");
        assert!(
            entry.remote_addr.is_none(),
            "LISTEN socket should have no remote_addr"
        );
    }

    #[test]
    fn test_skip_header_line() {
        let result = parse_proc_line(
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode",
            false,
            Protocol::Tcp,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_empty_line() {
        assert!(parse_proc_line("", false, Protocol::Tcp).is_none());
    }
}
