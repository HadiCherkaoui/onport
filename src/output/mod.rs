// Rust guideline compliant 2026-02-16

pub mod json;
pub mod table;
pub mod watch;

use anyhow::Result;
use std::net::IpAddr;

use crate::types::PortEntry;

/// Supported output formats.
pub enum OutputFormat {
    /// Pretty colored table for terminal display.
    Table,
    /// Structured JSON for scripting.
    Json,
}

/// Render port entries in the specified format.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn render(entries: &[PortEntry], format: &OutputFormat, no_color: bool) -> Result<()> {
    match format {
        OutputFormat::Table => table::render(entries, no_color),
        OutputFormat::Json => json::render(entries),
    }
}

/// Render extended process details below the main table.
///
/// Displays command line, start time, and open FD count in key-value format.
/// Fields that are `None` are omitted entirely.
pub fn render_details(details: &crate::types::ProcessDetails, _no_color: bool) {
    if let Some(cmdline) = &details.cmdline {
        println!("  Command:    {cmdline}");
    }
    if let Some(start_time) = &details.start_time {
        println!("  Started:    {start_time}");
    }
    if let Some(fd_count) = details.fd_count {
        println!("  Open FDs:   {fd_count}");
    }
    if let Some(tree) = &details.process_tree {
        println!("  Tree:       {tree}");
    }
}

/// Format a local bind address for display.
///
/// Unspecified addresses (`0.0.0.0` and `::`) are shown as `*` (all interfaces),
/// consistent with the `ss` convention.
pub(crate) fn format_address(addr: &IpAddr) -> String {
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
