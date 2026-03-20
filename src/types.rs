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
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
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

// from_hex is called only by the Linux platform module (dead on macOS and
// Windows); from_win_state is compiled only on Windows and calls back into
// the variants. Suppress dead_code for from_hex on all non-Linux platforms.
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

    /// Parse a Windows MIB TCP state code to `SocketState`.
    ///
    /// Maps Win32 `MIB_TCP_STATE_*` constants to `SocketState` variants.
    #[cfg(target_os = "windows")]
    pub fn from_win_state(state: u32) -> Self {
        // Windows MIB_TCP_STATE constants (from mstcpip.h / iprtrmib.h)
        const CLOSED: u32 = 1;
        const LISTEN: u32 = 2;
        const SYN_SENT: u32 = 3;
        const SYN_RCVD: u32 = 4;
        const ESTAB: u32 = 5;
        const FIN_WAIT1: u32 = 6;
        const FIN_WAIT2: u32 = 7;
        const CLOSE_WAIT: u32 = 8;
        const CLOSING: u32 = 9;
        const LAST_ACK: u32 = 10;
        const TIME_WAIT: u32 = 11;
        const DELETE_TCB: u32 = 12;

        match state {
            CLOSED | DELETE_TCB => SocketState::Close,
            LISTEN => SocketState::Listen,
            SYN_SENT => SocketState::SynSent,
            SYN_RCVD => SocketState::SynRecv,
            ESTAB => SocketState::Established,
            FIN_WAIT1 | FIN_WAIT2 => SocketState::Other("FIN_WAIT".to_string()),
            CLOSE_WAIT => SocketState::CloseWait,
            CLOSING | LAST_ACK => SocketState::Other("CLOSING".to_string()),
            TIME_WAIT => SocketState::TimeWait,
            other => SocketState::Other(format!("UNKNOWN({other})")),
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

    #[cfg(target_os = "windows")]
    #[test]
    fn test_socket_state_from_win_state() {
        assert_eq!(SocketState::from_win_state(2), SocketState::Listen);
        assert_eq!(SocketState::from_win_state(5), SocketState::Established);
        assert_eq!(SocketState::from_win_state(11), SocketState::TimeWait);
        assert_eq!(SocketState::from_win_state(8), SocketState::CloseWait);
        assert_eq!(SocketState::from_win_state(1), SocketState::Close);
        assert_eq!(SocketState::from_win_state(3), SocketState::SynSent);
        assert_eq!(SocketState::from_win_state(4), SocketState::SynRecv);
    }
}
