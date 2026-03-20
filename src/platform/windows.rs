// Rust guideline compliant 2026-02-16

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Result, bail};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
    MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_UDP6ROW_OWNER_PID,
    MIB_UDP6TABLE_OWNER_PID, MIB_UDPROW_OWNER_PID, MIB_UDPTABLE_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};

use super::PlatformProvider;
use crate::types::{PortEntry, Protocol, SocketState};

/// IPv4 address family constant.
///
/// AF_INET (2) is not exposed in the enabled windows-rs features, so we
/// define it as a literal. Value is stable across all Windows versions.
const AF_INET: u32 = 2;

/// IPv6 address family constant.
///
/// AF_INET6 (23) is not exposed in the enabled windows-rs features, so we
/// define it as a literal. Value is stable across all Windows versions.
const AF_INET6: u32 = 23;

/// Windows socket provider using Win32 `iphlpapi`.
///
/// Enumerates TCP and UDP sockets (IPv4 + IPv6) via
/// `GetExtendedTcpTable` / `GetExtendedUdpTable`, then resolves
/// process names and users via the `sysinfo` crate.
pub struct WindowsProvider;

impl PlatformProvider for WindowsProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        let mut entries = Vec::new();

        // Collect sockets from all four tables.
        // Each function gracefully returns an empty vec on failure.
        entries.extend(get_tcp_table_v4().unwrap_or_default());
        entries.extend(get_tcp_table_v6().unwrap_or_default());
        entries.extend(get_udp_table_v4().unwrap_or_default());
        entries.extend(get_udp_table_v6().unwrap_or_default());

        resolve_process_info(&mut entries);

        Ok(entries)
    }
}

// ── TCP IPv4 ────────────────────────────────────────────────────────────────

/// Enumerate IPv4 TCP sockets with owning PIDs.
///
/// Uses `GetExtendedTcpTable` with `TCP_TABLE_OWNER_PID_ALL` and `AF_INET`.
/// Returns an empty vec on failure (graceful degradation).
///
/// # Errors
///
/// Returns an error if the Win32 API call fails with an unexpected error code.
fn get_tcp_table_v4() -> Result<Vec<PortEntry>> {
    let mut size: u32 = 0;

    // Safety: first call with None buffer queries the required buffer size.
    // GetExtendedTcpTable writes the required size into `size` and returns
    // ERROR_INSUFFICIENT_BUFFER (122) when no output buffer is provided.
    let ret = unsafe {
        GetExtendedTcpTable(None, &mut size, false, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0)
    };
    if ret != ERROR_INSUFFICIENT_BUFFER.0 && ret != NO_ERROR.0 {
        bail!("GetExtendedTcpTable size query failed: error {ret}");
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];

    // Safety: buffer is sized to the value returned by the first call above.
    // GetExtendedTcpTable writes a MIB_TCPTABLE_OWNER_PID header followed by
    // `dwNumEntries` contiguous MIB_TCPROW_OWNER_PID rows into the buffer.
    let ret = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if ret != NO_ERROR.0 {
        bail!("GetExtendedTcpTable data query failed: error {ret}");
    }

    // Safety: buffer contains a valid MIB_TCPTABLE_OWNER_PID written by the
    // successful second call. The table field is a flexible array member;
    // from_raw_parts is valid for dwNumEntries elements because the buffer
    // was sized by the API to hold exactly that many rows.
    let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    let num_entries = table.dwNumEntries as usize;
    let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), num_entries) };

    Ok(rows.iter().map(tcp_v4_row_to_entry).collect())
}

/// Convert an IPv4 TCP row to a `PortEntry`.
fn tcp_v4_row_to_entry(row: &MIB_TCPROW_OWNER_PID) -> PortEntry {
    let local_ip = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
    let local_port = u16::from_be(row.dwLocalPort as u16);
    let remote_ip = Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes());
    let remote_port = u16::from_be(row.dwRemotePort as u16);

    let remote_addr = if remote_port != 0 || !remote_ip.is_unspecified() {
        Some(SocketAddr::new(IpAddr::V4(remote_ip), remote_port))
    } else {
        None
    };

    PortEntry {
        port: local_port,
        protocol: Protocol::Tcp,
        state: SocketState::from_win_state(row.dwState),
        pid: if row.dwOwningPid == 0 { None } else { Some(row.dwOwningPid) },
        process_name: None,
        user: None,
        local_addr: IpAddr::V4(local_ip),
        remote_addr,
        docker_container: None,
    }
}

// ── TCP IPv6 ────────────────────────────────────────────────────────────────

/// Enumerate IPv6 TCP sockets with owning PIDs.
///
/// Uses `GetExtendedTcpTable` with `TCP_TABLE_OWNER_PID_ALL` and `AF_INET6`.
/// Returns an empty vec on failure (graceful degradation).
///
/// # Errors
///
/// Returns an error if the Win32 API call fails with an unexpected error code.
fn get_tcp_table_v6() -> Result<Vec<PortEntry>> {
    let mut size: u32 = 0;

    // Safety: first call with None buffer queries the required buffer size.
    let ret = unsafe {
        GetExtendedTcpTable(None, &mut size, false, AF_INET6, TCP_TABLE_OWNER_PID_ALL, 0)
    };
    if ret != ERROR_INSUFFICIENT_BUFFER.0 && ret != NO_ERROR.0 {
        bail!("GetExtendedTcpTable(v6) size query failed: error {ret}");
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];

    // Safety: buffer is sized by the first call; the API writes a valid
    // MIB_TCP6TABLE_OWNER_PID structure followed by dwNumEntries rows.
    let ret = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET6,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if ret != NO_ERROR.0 {
        bail!("GetExtendedTcpTable(v6) data query failed: error {ret}");
    }

    // Safety: same invariant as the IPv4 case — buffer holds a valid
    // MIB_TCP6TABLE_OWNER_PID written by the API, with dwNumEntries rows.
    let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID) };
    let num_entries = table.dwNumEntries as usize;
    let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), num_entries) };

    Ok(rows.iter().map(tcp_v6_row_to_entry).collect())
}

/// Convert an IPv6 TCP row to a `PortEntry`.
fn tcp_v6_row_to_entry(row: &MIB_TCP6ROW_OWNER_PID) -> PortEntry {
    let local_ip = Ipv6Addr::from(row.ucLocalAddr);
    let local_port = u16::from_be(row.dwLocalPort as u16);
    let remote_ip = Ipv6Addr::from(row.ucRemoteAddr);
    let remote_port = u16::from_be(row.dwRemotePort as u16);

    let remote_addr = if remote_port != 0 || !remote_ip.is_unspecified() {
        Some(SocketAddr::new(IpAddr::V6(remote_ip), remote_port))
    } else {
        None
    };

    PortEntry {
        port: local_port,
        protocol: Protocol::Tcp,
        state: SocketState::from_win_state(row.dwState),
        pid: if row.dwOwningPid == 0 { None } else { Some(row.dwOwningPid) },
        process_name: None,
        user: None,
        local_addr: IpAddr::V6(local_ip),
        remote_addr,
        docker_container: None,
    }
}

// ── UDP IPv4 ────────────────────────────────────────────────────────────────

/// Enumerate IPv4 UDP sockets with owning PIDs.
///
/// Uses `GetExtendedUdpTable` with `UDP_TABLE_OWNER_PID` and `AF_INET`.
/// UDP has no state or remote address fields; all entries use `SocketState::Listen`.
/// Returns an empty vec on failure (graceful degradation).
///
/// # Errors
///
/// Returns an error if the Win32 API call fails with an unexpected error code.
fn get_udp_table_v4() -> Result<Vec<PortEntry>> {
    let mut size: u32 = 0;

    // Safety: first call with None buffer queries the required buffer size.
    let ret =
        unsafe { GetExtendedUdpTable(None, &mut size, false, AF_INET, UDP_TABLE_OWNER_PID, 0) };
    if ret != ERROR_INSUFFICIENT_BUFFER.0 && ret != NO_ERROR.0 {
        bail!("GetExtendedUdpTable size query failed: error {ret}");
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];

    // Safety: buffer is sized by the first call; the API writes a valid
    // MIB_UDPTABLE_OWNER_PID structure followed by dwNumEntries rows.
    let ret = unsafe {
        GetExtendedUdpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if ret != NO_ERROR.0 {
        bail!("GetExtendedUdpTable data query failed: error {ret}");
    }

    // Safety: buffer holds a valid MIB_UDPTABLE_OWNER_PID written by the API.
    let table = unsafe { &*(buffer.as_ptr() as *const MIB_UDPTABLE_OWNER_PID) };
    let num_entries = table.dwNumEntries as usize;
    let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), num_entries) };

    Ok(rows.iter().map(udp_v4_row_to_entry).collect())
}

/// Convert an IPv4 UDP row to a `PortEntry`.
fn udp_v4_row_to_entry(row: &MIB_UDPROW_OWNER_PID) -> PortEntry {
    PortEntry {
        port: u16::from_be(row.dwLocalPort as u16),
        protocol: Protocol::Udp,
        // UDP has no connection state; treat all bound sockets as listening.
        state: SocketState::Listen,
        pid: if row.dwOwningPid == 0 { None } else { Some(row.dwOwningPid) },
        process_name: None,
        user: None,
        local_addr: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
        remote_addr: None,
        docker_container: None,
    }
}

// ── UDP IPv6 ────────────────────────────────────────────────────────────────

/// Enumerate IPv6 UDP sockets with owning PIDs.
///
/// Uses `GetExtendedUdpTable` with `UDP_TABLE_OWNER_PID` and `AF_INET6`.
/// Returns an empty vec on failure (graceful degradation).
///
/// # Errors
///
/// Returns an error if the Win32 API call fails with an unexpected error code.
fn get_udp_table_v6() -> Result<Vec<PortEntry>> {
    let mut size: u32 = 0;

    // Safety: first call with None buffer queries the required buffer size.
    let ret =
        unsafe { GetExtendedUdpTable(None, &mut size, false, AF_INET6, UDP_TABLE_OWNER_PID, 0) };
    if ret != ERROR_INSUFFICIENT_BUFFER.0 && ret != NO_ERROR.0 {
        bail!("GetExtendedUdpTable(v6) size query failed: error {ret}");
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];

    // Safety: buffer is sized by the first call; the API writes a valid
    // MIB_UDP6TABLE_OWNER_PID structure followed by dwNumEntries rows.
    let ret = unsafe {
        GetExtendedUdpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET6,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if ret != NO_ERROR.0 {
        bail!("GetExtendedUdpTable(v6) data query failed: error {ret}");
    }

    // Safety: buffer holds a valid MIB_UDP6TABLE_OWNER_PID written by the API.
    let table = unsafe { &*(buffer.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID) };
    let num_entries = table.dwNumEntries as usize;
    let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), num_entries) };

    Ok(rows.iter().map(udp_v6_row_to_entry).collect())
}

/// Convert an IPv6 UDP row to a `PortEntry`.
fn udp_v6_row_to_entry(row: &MIB_UDP6ROW_OWNER_PID) -> PortEntry {
    PortEntry {
        port: u16::from_be(row.dwLocalPort as u16),
        protocol: Protocol::Udp,
        // UDP has no connection state; treat all bound sockets as listening.
        state: SocketState::Listen,
        pid: if row.dwOwningPid == 0 { None } else { Some(row.dwOwningPid) },
        process_name: None,
        user: None,
        local_addr: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
        remote_addr: None,
        docker_container: None,
    }
}

// ── Process resolution ───────────────────────────────────────────────────────

/// Resolve process names and usernames for entries using `sysinfo`.
///
/// Creates a `sysinfo::System`, refreshes only the PIDs found in `entries`,
/// and fills in `process_name` and `user` fields.
fn resolve_process_info(entries: &mut [PortEntry]) {
    use std::collections::HashSet;

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

    let unique_pids: Vec<Pid> = entries
        .iter()
        .filter_map(|e| e.pid)
        .collect::<HashSet<_>>()
        .into_iter()
        .map(Pid::from_u32)
        .collect();

    if unique_pids.is_empty() {
        return;
    }

    // Refresh only the PIDs we need; include user info for username resolution.
    let refresh_kind = ProcessRefreshKind::nothing().with_user(UpdateKind::Always);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&unique_pids),
        true,
        refresh_kind,
    );

    let users = Users::new_with_refreshed_list();

    for entry in entries.iter_mut() {
        let Some(pid) = entry.pid else { continue };
        let Some(process) = system.process(Pid::from_u32(pid)) else { continue };

        entry.process_name = Some(process.name().to_string_lossy().into_owned());

        if let Some(uid) = process.user_id()
            && let Some(user) = users.get_user_by_id(uid)
        {
            entry.user = Some(user.name().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_byte_order() {
        // Port 80 in network byte order stored in the low 16 bits of a u32:
        // 80 = 0x0050, big-endian bytes = [0x00, 0x50] → stored as 0x5000 on LE.
        let raw: u32 = 0x5000;
        assert_eq!(u16::from_be(raw as u16), 80);

        // Port 443 = 0x01BB, big-endian bytes = [0x01, 0xBB] → stored as 0xBB01 on LE.
        let raw443: u32 = 0xBB01;
        assert_eq!(u16::from_be(raw443 as u16), 443);
    }

    #[test]
    fn test_ipv4_conversion() {
        // 127.0.0.1 in network byte order as u32 on little-endian = 0x0100007F.
        let raw: u32 = 0x0100_007F;
        let ip = Ipv4Addr::from(raw.to_ne_bytes());
        assert_eq!(ip, Ipv4Addr::new(127, 0, 0, 1));
    }
}
