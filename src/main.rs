// Rust guideline compliant 2026-02-16

mod docker;
mod kill;
mod output;
mod platform;
mod types;

use anyhow::{Context, Result};
use clap::Parser;
use mimalloc::MiMalloc;

use output::OutputFormat;

/// Use mimalloc as the global allocator for improved throughput.
///
/// Applications should prefer mimalloc; we have observed up to 25%
/// benchmark improvements along allocating hot paths.
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// See what's listening on your ports.
#[derive(Parser)]
#[command(name = "onport", version, about)]
struct Cli {
    /// Port numbers to filter (e.g., 3000 8080 or :3000 :8080).
    ports: Vec<String>,

    /// Show only TCP sockets.
    #[arg(long)]
    tcp: bool,

    /// Show only UDP sockets.
    #[arg(long)]
    udp: bool,

    /// Show all socket states, not just LISTEN.
    #[arg(long)]
    all: bool,

    /// Output as JSON for scripting.
    #[arg(long)]
    json: bool,

    /// Disable colored output.
    #[arg(long)]
    no_color: bool,

    /// Kill the process on the specified port.
    #[arg(short = 'k', long = "kill")]
    kill: bool,

    /// Force kill (SIGKILL) without confirmation. Only effective with --kill.
    #[arg(long = "force", short = 'f')]
    force: bool,

    /// Live-updating watch mode (refresh every 2s).
    #[arg(short = 'w', long = "watch", conflicts_with_all = ["kill", "json"])]
    watch: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let port_filters = parse_port_filters(&cli.ports)?;

    // Kill mode requires a specific port to avoid ambiguity.
    if cli.kill && port_filters.is_empty() {
        eprintln!("Error: --kill requires a port number (e.g., onport --kill 3000).");
        return Ok(());
    }

    let provider = platform::get_provider();

    // Watch mode — enter the live-update loop and return when the user quits.
    if cli.watch {
        let protocol_filter = if cli.tcp && !cli.udp {
            Some(types::Protocol::Tcp)
        } else if cli.udp && !cli.tcp {
            Some(types::Protocol::Udp)
        } else {
            None
        };
        return output::watch::run_watch(
            provider.as_ref(),
            &port_filters,
            protocol_filter,
            cli.all,
            cli.no_color,
        );
    }

    let mut entries = provider
        .list_sockets()
        .context("Failed to enumerate sockets")?;

    // Filter by port if specified
    if !port_filters.is_empty() {
        entries.retain(|e| port_filters.contains(&e.port));
    }

    // Filter by protocol
    if cli.tcp && !cli.udp {
        entries.retain(|e| e.protocol == types::Protocol::Tcp);
    } else if cli.udp && !cli.tcp {
        entries.retain(|e| e.protocol == types::Protocol::Udp);
    }

    // Filter by state: show only LISTEN unless --all
    if !cli.all {
        entries.retain(|e| e.state == types::SocketState::Listen);
    }

    // Deduplicate wildcard IPv4/IPv6 entries that represent the same socket
    dedup_entries(&mut entries);

    // Sort by port number
    entries.sort_by_key(|e| e.port);

    // Enrich entries with Docker container names where ports match.
    docker::enrich_with_docker(&mut entries);

    // Handle kill mode
    if cli.kill {
        if entries.is_empty() {
            eprintln!("No process found on the specified port(s).");
            return Ok(());
        }
        if entries.len() > 1 {
            // Show the table so user knows what matched, then error
            output::render(&entries, &OutputFormat::Table, cli.no_color)?;
            eprintln!("Multiple processes found. Specify a single port.");
            return Ok(());
        }
        kill::kill_process(&entries[0], cli.force)?;
        return Ok(());
    }

    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    };

    output::render(&entries, &format, cli.no_color)?;

    Ok(())
}

/// Remove duplicate socket entries that represent the same logical socket.
///
/// Windows returns separate IPv4 (`0.0.0.0`) and IPv6 (`::`) entries for
/// dual-stack or wildcard-bound sockets. Both map to `"*"` in the ADDRESS
/// column. This function retains only the first occurrence per logical socket,
/// treating all unspecified addresses as equivalent.
///
/// Dedup key: `(port, protocol, pid, state, normalized_local_addr, remote_addr)`
/// where `normalized_local_addr` is `None` for any unspecified address.
pub(crate) fn dedup_entries(entries: &mut Vec<types::PortEntry>) {
    use std::collections::HashSet;
    use std::net::IpAddr;

    let mut seen = HashSet::new();
    entries.retain(|e| {
        let norm_addr: Option<IpAddr> = if e.local_addr.is_unspecified() {
            None
        } else {
            Some(e.local_addr)
        };
        seen.insert((e.port, e.protocol.clone(), e.pid, e.state.clone(), norm_addr, e.remote_addr))
    });
}

/// Parse port filter arguments, stripping optional `:` prefix.
///
/// # Errors
///
/// Returns an error if a port argument is not a valid u16 number.
fn parse_port_filters(args: &[String]) -> Result<Vec<u16>> {
    args.iter()
        .map(|arg| {
            let cleaned = arg.strip_prefix(':').unwrap_or(arg);
            cleaned
                .parse::<u16>()
                .with_context(|| format!("Invalid port number: {arg}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_entries_removes_wildcard_duplicates() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        fn make_entry(port: u16, addr: IpAddr) -> PortEntry {
            PortEntry {
                port,
                protocol: Protocol::Tcp,
                state: SocketState::Listen,
                pid: Some(1234),
                process_name: None,
                user: None,
                local_addr: addr,
                remote_addr: None,
                docker_container: None,
            }
        }

        let mut entries = vec![
            make_entry(80, IpAddr::V4(Ipv4Addr::UNSPECIFIED)), // 0.0.0.0
            make_entry(80, IpAddr::V6(Ipv6Addr::UNSPECIFIED)), // :: (duplicate)
            make_entry(80, IpAddr::V4(Ipv4Addr::LOCALHOST)),   // 127.0.0.1 (distinct)
            make_entry(443, IpAddr::V4(Ipv4Addr::UNSPECIFIED)), // different port, kept
        ];

        dedup_entries(&mut entries);

        // Expect: 80/IPv4-wildcard, 80/localhost, 443/wildcard (IPv6 wildcard dropped)
        assert_eq!(entries.len(), 3, "expected 3 entries after dedup");
        assert_eq!(entries[0].local_addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(entries[1].local_addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn test_parse_port_filters_bare_numbers() {
        let args = vec!["3000".to_string(), "8080".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![3000, 8080]);
    }

    #[test]
    fn test_parse_port_filters_with_colon() {
        let args = vec![":3000".to_string(), ":8080".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![3000, 8080]);
    }

    #[test]
    fn test_parse_port_filters_mixed() {
        let args = vec!["3000".to_string(), ":8080".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![3000, 8080]);
    }

    #[test]
    fn test_parse_port_filters_empty() {
        let args: Vec<String> = vec![];
        let result = parse_port_filters(&args).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_port_filters_invalid() {
        let args = vec!["not_a_port".to_string()];
        assert!(parse_port_filters(&args).is_err());
    }
}
