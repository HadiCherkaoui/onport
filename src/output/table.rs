// Rust guideline compliant 2026-02-16

use anyhow::Result;
use owo_colors::OwoColorize;
use std::net::IpAddr;
use tabled::settings::object::Columns;
use tabled::settings::{Alignment, Style};
use tabled::{Table, Tabled};

use crate::types::{PortEntry, SocketState};

/// A single row in the output table.
#[derive(Tabled)]
struct TableRow {
    #[tabled(rename = "PORT")]
    port: String,
    #[tabled(rename = "PROTO")]
    proto: String,
    #[tabled(rename = "ADDRESS")]
    address: String,
    #[tabled(rename = "PROCESS")]
    process: String,
    #[tabled(rename = "PID")]
    pid: String,
    #[tabled(rename = "USER")]
    user: String,
    #[tabled(rename = "STATE")]
    state: String,
}

/// Render port entries as a colored, aligned table.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn render(entries: &[PortEntry], no_color: bool) -> Result<()> {
    if entries.is_empty() {
        println!("No matching sockets found.");
        return Ok(());
    }

    let rows: Vec<TableRow> = entries
        .iter()
        .map(|e| {
            let process_name = e.process_name.as_deref().unwrap_or("?");
            // Truncate process names to 16 characters for alignment
            let process_display = if process_name.len() > 16 {
                format!("{}…", &process_name[..15])
            } else {
                process_name.to_string()
            };

            let state_str = e.state.to_string();
            let state_display = if no_color {
                state_str
            } else {
                colorize_state(&e.state)
            };

            let docker_suffix = e
                .docker_container
                .as_ref()
                .map(|name| format!("  [docker: {name}]"))
                .unwrap_or_default();

            TableRow {
                port: e.port.to_string(),
                proto: e.protocol.to_string(),
                address: format_address(&e.local_addr),
                process: process_display,
                pid: e.pid.map_or_else(|| "?".to_string(), |p| p.to_string()),
                user: e.user.clone().unwrap_or_else(|| "?".to_string()),
                state: format!("{state_display}{docker_suffix}"),
            }
        })
        .collect();

    let mut table = Table::new(&rows);
    table
        .with(Style::blank())
        .modify(Columns::first(), Alignment::right());

    println!("{table}");
    Ok(())
}

/// Apply color to a socket state string.
fn colorize_state(state: &SocketState) -> String {
    let text = state.to_string();
    match state {
        SocketState::Listen => text.green().to_string(),
        SocketState::Established => text.yellow().to_string(),
        SocketState::TimeWait | SocketState::CloseWait => text.red().to_string(),
        _ => text,
    }
}

/// Format an IP address for display, showing `*` for unspecified addresses.
///
/// Converts `0.0.0.0` (IPv4) and `::` (IPv6) to `*` to indicate listening on all interfaces,
/// consistent with `ss` convention. All other addresses are displayed as-is.
fn format_address(addr: &IpAddr) -> String {
    match addr {
        IpAddr::V4(v4) if v4.is_unspecified() => "*".to_string(),
        IpAddr::V6(v6) if v6.is_unspecified() => "*".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_format_address_ipv4_unspecified() {
        let addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        assert_eq!(format_address(&addr), "*");
    }

    #[test]
    fn test_format_address_ipv6_unspecified() {
        let addr = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        assert_eq!(format_address(&addr), "*");
    }

    #[test]
    fn test_format_address_specific() {
        let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(format_address(&addr), "127.0.0.1");
    }
}
