// Rust guideline compliant 2026-02-16

use std::fmt;
use std::net::{IpAddr, SocketAddr};

use serde::Serialize;

/// Represents a single network socket entry.
///
/// Contains all information about a listening or connected socket,
/// including the owning process, user, and optional Docker container.
#[derive(Debug, Clone, Serialize)]
pub struct PortEntry {
    /// Local port number.
    pub port: u16,
    /// Transport protocol (TCP or UDP).
    pub protocol: Protocol,
    /// Current socket state.
    pub state: SocketState,
    /// PID of the owning process, if resolvable.
    pub pid: Option<u32>,
    /// Name of the owning process, if resolvable.
    pub process_name: Option<String>,
    /// Username of the owning process, if resolvable.
    pub user: Option<String>,
    /// Local bind address.
    pub local_addr: IpAddr,
    /// Remote address for established connections.
    pub remote_addr: Option<SocketAddr>,
    /// Docker container name, if the process is containerized.
    pub docker_container: Option<String>,
}

/// Transport protocol type.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Protocol {
    /// Transmission Control Protocol.
    Tcp,
    /// User Datagram Protocol.
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "tcp"),
            Protocol::Udp => write!(f, "udp"),
        }
    }
}

/// Socket connection state.
///
/// Maps to standard TCP states. UDP sockets are always `Listen`.
// Variants are constructed by the Linux platform module; allow dead_code on
// other platforms where that module is not compiled in.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum SocketState {
    /// Socket is listening for incoming connections.
    Listen,
    /// Connection is established.
    Established,
    /// Waiting for remote TCP connection termination.
    TimeWait,
    /// Waiting for local user to close the connection.
    CloseWait,
    /// SYN has been sent, waiting for SYN-ACK.
    SynSent,
    /// SYN-ACK received, waiting for final ACK.
    SynRecv,
    /// Connection is closed.
    Close,
    /// Any other state not explicitly tracked.
    Other(String),
}

impl fmt::Display for SocketState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketState::Listen => write!(f, "LISTEN"),
            SocketState::Established => write!(f, "ESTABLISHED"),
            SocketState::TimeWait => write!(f, "TIME_WAIT"),
            SocketState::CloseWait => write!(f, "CLOSE_WAIT"),
            SocketState::SynSent => write!(f, "SYN_SENT"),
            SocketState::SynRecv => write!(f, "SYN_RECV"),
            SocketState::Close => write!(f, "CLOSE"),
            SocketState::Other(s) => write!(f, "{s}"),
        }
    }
}

// from_hex is called by the Linux platform module; allow dead_code on other
// platforms where that module is not compiled in.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl SocketState {
    /// Parse a hex state code from `/proc/net/tcp`.
    ///
    /// Maps Linux kernel TCP state codes to `SocketState` variants.
    /// Unknown codes become `Other` with the raw hex string.
    pub fn from_hex(code: &str) -> Self {
        match code.to_uppercase().as_str() {
            "01" => SocketState::Established,
            "02" => SocketState::SynSent,
            "03" => SocketState::SynRecv,
            "06" => SocketState::TimeWait,
            "07" => SocketState::Close,
            "08" => SocketState::CloseWait,
            "0A" => SocketState::Listen,
            other => SocketState::Other(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_state_from_hex() {
        assert_eq!(SocketState::from_hex("0A"), SocketState::Listen);
        assert_eq!(SocketState::from_hex("01"), SocketState::Established);
        assert_eq!(SocketState::from_hex("06"), SocketState::TimeWait);
        assert_eq!(SocketState::from_hex("08"), SocketState::CloseWait);
        assert_eq!(SocketState::from_hex("0a"), SocketState::Listen); // lowercase
        assert_eq!(SocketState::from_hex("FF"), SocketState::Other("FF".to_string()));
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(Protocol::Tcp.to_string(), "tcp");
        assert_eq!(Protocol::Udp.to_string(), "udp");
    }

    #[test]
    fn test_socket_state_display() {
        assert_eq!(SocketState::Listen.to_string(), "LISTEN");
        assert_eq!(SocketState::Established.to_string(), "ESTABLISHED");
        assert_eq!(SocketState::TimeWait.to_string(), "TIME_WAIT");
        assert_eq!(SocketState::Other("UNKNOWN".to_string()).to_string(), "UNKNOWN");
    }
}
