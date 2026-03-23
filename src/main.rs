// Rust guideline compliant 2026-02-16

mod docker;
mod kill;
mod output;
mod platform;
mod process_detail;
mod types;

use std::io::{IsTerminal, Write as _};

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

/// Field to sort port entries by.
#[derive(Clone, Default, clap::ValueEnum)]
pub(crate) enum SortField {
    /// Sort by port number (default).
    #[default]
    Port,
    /// Sort by PID.
    Pid,
    /// Sort by process name (case-insensitive).
    Name,
    /// Sort by username.
    User,
    /// Sort by socket state.
    State,
    /// Sort by protocol.
    Proto,
}

/// See what's listening on your ports.
#[derive(Parser)]
#[command(name = "onport", version, about)]
struct Cli {
    /// Port numbers or ranges to filter (e.g., 3000 8080 3000-3002 :3000).
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

    /// Disable Docker container name detection.
    #[arg(long)]
    no_docker: bool,

    /// Kill the process on the specified port.
    #[arg(short = 'k', long = "kill")]
    kill: bool,

    /// Force kill (SIGKILL) without confirmation. Only effective with --kill.
    #[arg(long = "force", short = 'f')]
    force: bool,

    /// Filter by process name (case-insensitive substring match).
    #[arg(short = 'n', long = "name")]
    name: Option<String>,

    /// Filter by PID.
    #[arg(long = "pid")]
    pid: Option<u32>,

    /// Show only IPv4 sockets.
    #[arg(short = '4', long = "ipv4")]
    ipv4: bool,

    /// Show only IPv6 sockets.
    #[arg(short = '6', long = "ipv6")]
    ipv6: bool,

    /// Field to sort results by.
    #[arg(long = "sort", value_enum, default_value_t = SortField::Port)]
    sort: SortField,

    /// Disable process name truncation.
    #[arg(short = 'W', long = "wide")]
    wide: bool,

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
        let opts = output::watch::WatchOptions {
            port_filters: &port_filters,
            protocol_filter,
            show_all_states: cli.all,
            no_color: cli.no_color,
            no_docker: cli.no_docker,
            name_filter: cli.name.as_deref(),
            pid_filter: cli.pid,
            ipv4_only: cli.ipv4 && !cli.ipv6,
            ipv6_only: cli.ipv6 && !cli.ipv4,
            sort_field: &cli.sort,
            wide: cli.wide,
        };
        return output::watch::run_watch(provider.as_ref(), &opts);
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

    // Filter by process name (case-insensitive substring)
    if let Some(ref name_filter) = cli.name {
        let lower = name_filter.to_lowercase();
        entries.retain(|e| {
            e.process_name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        });
    }

    // Filter by PID
    if let Some(pid_filter) = cli.pid {
        entries.retain(|e| e.pid == Some(pid_filter));
    }

    // Filter by IP version
    if cli.ipv4 && !cli.ipv6 {
        entries.retain(|e| e.local_addr.is_ipv4());
    } else if cli.ipv6 && !cli.ipv4 {
        entries.retain(|e| e.local_addr.is_ipv6());
    }

    // Deduplicate wildcard IPv4/IPv6 entries that represent the same socket
    dedup_entries(&mut entries);

    // Sort entries by the requested field.
    apply_sort(&mut entries, &cli.sort);

    // Enrich entries with Docker container names where ports match.
    if !cli.no_docker {
        docker::enrich_with_docker(&mut entries);
    }

    // Handle kill mode
    if cli.kill {
        if entries.is_empty() {
            eprintln!("No process found on the specified port(s).");
            return Ok(());
        }
        // Allow killing when all entries share the same process name (e.g. docker-proxy
        // spawning separate IPv4/IPv6 listeners). Reject only when genuinely different
        // processes would be affected.
        if !is_single_process(&entries) {
            output::render(&entries, &OutputFormat::Table, &output::RenderOptions { no_color: cli.no_color, wide: cli.wide })?;
            eprintln!("Multiple different processes found. Specify a single port.");
            return Ok(());
        }
        kill::kill_processes(&entries, cli.force)?;
        return Ok(());
    }

    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    };

    // Build a deduplicated copy for display. The original `entries` is kept
    // intact so the enhanced single-port view and kill path see all PIDs.
    let mut display_entries = entries.clone();
    dedup_same_service(&mut display_entries);

    output::render(&display_entries, &format, &output::RenderOptions { no_color: cli.no_color, wide: cli.wide })?;

    // Enhanced single-port view: show process details and offer an inline kill prompt
    // when exactly one port is queried, a single process matched, and we are in a TTY.
    let is_single_port_view = port_filters.len() == 1
        && !cli.json
        && std::io::stdout().is_terminal();

    if is_single_port_view {
        let unique_pids: Vec<u32> = entries
            .iter()
            .filter_map(|e| e.pid)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if unique_pids.len() == 1 {
            let details = process_detail::resolve(unique_pids[0]);
            output::render_details(&details);

            println!();
            print!("  Kill this process? [y/N] ");
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            if line.starts_with('y') || line.starts_with('Y') {
                // User already confirmed via the inline prompt; skip the second
                // confirmation that kill_processes would show.
                kill::kill_confirmed(&entries)?;
            }
        }
    }

    Ok(())
}

/// Sort port entries by the given field.
pub(crate) fn apply_sort(entries: &mut [crate::types::PortEntry], field: &SortField) {
    match field {
        SortField::Port  => entries.sort_by_key(|e| e.port),
        SortField::Pid   => entries.sort_by_key(|e| e.pid.unwrap_or(0)),
        SortField::Name  => entries.sort_by(|a, b| {
            a.process_name.as_deref().unwrap_or("").to_lowercase()
                .cmp(&b.process_name.as_deref().unwrap_or("").to_lowercase())
        }),
        SortField::User  => entries.sort_by(|a, b| {
            a.user.as_deref().unwrap_or("").cmp(b.user.as_deref().unwrap_or(""))
        }),
        SortField::State => entries.sort_by(|a, b| {
            a.state.to_string().cmp(&b.state.to_string())
        }),
        SortField::Proto => entries.sort_by(|a, b| {
            a.protocol.to_string().cmp(&b.protocol.to_string())
        }),
    }
}

/// Return `true` when all entries share the same process name (one logical service).
///
/// Two processes cannot bind the same port, so entries filtered to the same
/// port(s) with the same name always represent a single logical service (e.g.
/// docker-proxy with separate IPv4/IPv6 listeners).
pub(crate) fn is_single_process(entries: &[types::PortEntry]) -> bool {
    let unique_names: std::collections::HashSet<_> = entries
        .iter()
        .map(|e| e.process_name.as_deref().unwrap_or("?"))
        .collect();
    unique_names.len() <= 1
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

/// Collapse entries that represent the same logical service for display purposes.
///
/// docker-proxy spawns separate IPv4 and IPv6 listener processes for the same
/// port, producing two rows that are visually identical from the user's
/// perspective. This function collapses entries sharing the same
/// `(port, protocol, state, process_name)` tuple down to a single row.
///
/// **Display only** — this is called on a clone of entries. The original slice
/// is unchanged so that the kill path and enhanced detail view can still
/// operate on all PIDs.
pub(crate) fn dedup_same_service(entries: &mut Vec<types::PortEntry>) {
    use std::collections::HashSet;

    // Key: (port, protocol, state, process_name)
    let mut seen: HashSet<(u16, String, String, String)> = HashSet::new();
    entries.retain(|e| {
        let key = (
            e.port,
            e.protocol.to_string(),
            e.state.to_string(),
            e.process_name.as_deref().unwrap_or("?").to_string(),
        );
        seen.insert(key)
    });
}

/// Parse port filter arguments, supporting single ports and `N-M` ranges.
///
/// Each argument may optionally be prefixed with `:` (e.g. `:3000` or `:3000-3002`).
/// A `-` separator expands the argument into the inclusive range `start..=end`.
/// Mixed arguments work: `["80", "3000-3002"]` produces `[80, 3000, 3001, 3002]`.
///
/// # Errors
///
/// Returns an error if any port value is not a valid `u16`, or if the start of a
/// range is greater than its end (reversed range).
fn parse_port_filters(args: &[String]) -> Result<Vec<u16>> {
    let mut ports = Vec::new();
    for arg in args {
        let cleaned = arg.strip_prefix(':').unwrap_or(arg);
        if let Some((start_str, end_str)) = cleaned.split_once('-') {
            let start = start_str
                .parse::<u16>()
                .with_context(|| format!("Invalid port number in range: {arg}"))?;
            let end = end_str
                .parse::<u16>()
                .with_context(|| format!("Invalid port number in range: {arg}"))?;
            if start > end {
                anyhow::bail!("Invalid port range (start > end): {arg}");
            }
            ports.extend(start..=end);
        } else {
            let port = cleaned
                .parse::<u16>()
                .with_context(|| format!("Invalid port number: {arg}"))?;
            ports.push(port);
        }
    }
    Ok(ports)
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

    #[test]
    fn test_parse_port_filters_range() {
        let args = vec!["3000-3002".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![3000, 3001, 3002]);
    }

    #[test]
    fn test_parse_port_filters_range_with_colon() {
        let args = vec![":8080-8082".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![8080, 8081, 8082]);
    }

    #[test]
    fn test_parse_port_filters_single_port_range() {
        let args = vec!["9000-9000".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![9000]);
    }

    #[test]
    fn test_parse_port_filters_reversed_range_errors() {
        let args = vec!["9000-8000".to_string()];
        assert!(parse_port_filters(&args).is_err());
    }

    #[test]
    fn test_parse_port_filters_mixed_range_and_single() {
        let args = vec!["80".to_string(), "3000-3002".to_string()];
        let result = parse_port_filters(&args).unwrap();
        assert_eq!(result, vec![80, 3000, 3001, 3002]);
    }

    #[test]
    fn test_is_single_process_same_name() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        fn make_entry(addr: IpAddr, name: &str) -> PortEntry {
            PortEntry {
                port: 8888,
                protocol: Protocol::Tcp,
                state: SocketState::Listen,
                pid: Some(100),
                process_name: Some(name.to_string()),
                user: None,
                local_addr: addr,
                remote_addr: None,
                docker_container: None,
            }
        }

        // Both entries share the same process name → single logical service
        let entries = vec![
            make_entry(IpAddr::V4(Ipv4Addr::UNSPECIFIED), "docker-proxy"),
            make_entry(IpAddr::V6(Ipv6Addr::UNSPECIFIED), "docker-proxy"),
        ];
        assert!(is_single_process(&entries));
    }

    #[test]
    fn test_dedup_same_service_collapses_docker_proxy() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        fn make_proxy_entry(addr: IpAddr, pid: u32) -> PortEntry {
            PortEntry {
                port: 8888,
                protocol: Protocol::Tcp,
                state: SocketState::Listen,
                pid: Some(pid),
                process_name: Some("docker-proxy".to_string()),
                user: None,
                local_addr: addr,
                remote_addr: None,
                docker_container: None,
            }
        }

        // Two docker-proxy entries (IPv4 + IPv6), different PIDs
        let mut entries = vec![
            make_proxy_entry(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 1001),
            make_proxy_entry(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 1002),
        ];

        dedup_same_service(&mut entries);

        assert_eq!(entries.len(), 1, "two docker-proxy entries should collapse to one for display");
    }

    #[test]
    fn test_dedup_same_service_keeps_different_processes() {
        use std::net::{IpAddr, Ipv4Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        fn make_entry(port: u16, name: &str) -> PortEntry {
            PortEntry {
                port,
                protocol: Protocol::Tcp,
                state: SocketState::Listen,
                pid: Some(100),
                process_name: Some(name.to_string()),
                user: None,
                local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                remote_addr: None,
                docker_container: None,
            }
        }

        // Two different services on different ports — must not be collapsed
        let mut entries = vec![
            make_entry(80, "nginx"),
            make_entry(443, "nginx"),
        ];

        dedup_same_service(&mut entries);

        assert_eq!(entries.len(), 2, "different ports must not be collapsed");
    }

    #[test]
    fn test_is_single_process_different_names() {
        use std::net::{IpAddr, Ipv4Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        fn make_entry(port: u16, name: &str) -> PortEntry {
            PortEntry {
                port,
                protocol: Protocol::Tcp,
                state: SocketState::Listen,
                pid: Some(100),
                process_name: Some(name.to_string()),
                user: None,
                local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                remote_addr: None,
                docker_container: None,
            }
        }

        // Different process names → multiple distinct processes
        let entries = vec![
            make_entry(80, "nginx"),
            make_entry(80, "apache2"),
        ];
        assert!(!is_single_process(&entries));
    }

    // ── Name / PID filter tests ───────────────────────────────────────────────

    fn make_test_entry(port: u16, name: &str, pid: u32) -> crate::types::PortEntry {
        use std::net::{IpAddr, Ipv4Addr};
        use crate::types::{PortEntry, Protocol, SocketState};
        PortEntry {
            port,
            protocol: Protocol::Tcp,
            state: SocketState::Listen,
            pid: Some(pid),
            process_name: Some(name.to_string()),
            user: None,
            local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            remote_addr: None,
            docker_container: None,
        }
    }

    #[test]
    fn test_name_filter_case_insensitive() {
        let mut entries = vec![
            make_test_entry(80, "nginx", 100),
            make_test_entry(5432, "Postgres", 200),
        ];
        let lower = "NGI".to_lowercase();
        entries.retain(|e| {
            e.process_name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        });
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name.as_deref(), Some("nginx"));
    }

    #[test]
    fn test_name_filter_no_match() {
        let mut entries = vec![
            make_test_entry(80, "nginx", 100),
            make_test_entry(5432, "Postgres", 200),
        ];
        let lower = "nonexistent".to_lowercase();
        entries.retain(|e| {
            e.process_name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        });
        assert!(entries.is_empty());
    }

    #[test]
    fn test_pid_filter_match() {
        let mut entries = vec![
            make_test_entry(80, "nginx", 1234),
            make_test_entry(5432, "postgres", 5678),
        ];
        let pid_filter: u32 = 1234;
        entries.retain(|e| e.pid == Some(pid_filter));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pid, Some(1234));
    }

    #[test]
    fn test_pid_filter_no_match() {
        let mut entries = vec![
            make_test_entry(80, "nginx", 1234),
        ];
        let pid_filter: u32 = 9999;
        entries.retain(|e| e.pid == Some(pid_filter));
        assert!(entries.is_empty());
    }

    #[test]
    fn test_ipv4_filter_retains_only_ipv4() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        let mut entries = vec![
            PortEntry { port: 80, protocol: Protocol::Tcp, state: SocketState::Listen, pid: None,
                        process_name: None, user: None,
                        local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED), remote_addr: None, docker_container: None },
            PortEntry { port: 80, protocol: Protocol::Tcp, state: SocketState::Listen, pid: None,
                        process_name: None, user: None,
                        local_addr: IpAddr::V6(Ipv6Addr::UNSPECIFIED), remote_addr: None, docker_container: None },
        ];
        entries.retain(|e| e.local_addr.is_ipv4());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].local_addr.is_ipv4());
    }

    #[test]
    fn test_ipv6_filter_retains_only_ipv6() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use crate::types::{PortEntry, Protocol, SocketState};

        let mut entries = vec![
            PortEntry { port: 80, protocol: Protocol::Tcp, state: SocketState::Listen, pid: None,
                        process_name: None, user: None,
                        local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED), remote_addr: None, docker_container: None },
            PortEntry { port: 80, protocol: Protocol::Tcp, state: SocketState::Listen, pid: None,
                        process_name: None, user: None,
                        local_addr: IpAddr::V6(Ipv6Addr::UNSPECIFIED), remote_addr: None, docker_container: None },
        ];
        entries.retain(|e| e.local_addr.is_ipv6());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].local_addr.is_ipv6());
    }

    #[test]
    fn test_sort_by_pid() {
        use crate::types::{PortEntry, Protocol, SocketState};
        use std::net::{IpAddr, Ipv4Addr};

        fn make_entry(port: u16, pid: u32) -> PortEntry {
            PortEntry { port, protocol: Protocol::Tcp, state: SocketState::Listen,
                        pid: Some(pid), process_name: None, user: None,
                        local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED), remote_addr: None, docker_container: None }
        }

        let mut entries = vec![make_entry(443, 200), make_entry(80, 100), make_entry(8080, 300)];
        apply_sort(&mut entries, &SortField::Pid);
        assert_eq!(entries[0].pid, Some(100));
        assert_eq!(entries[1].pid, Some(200));
        assert_eq!(entries[2].pid, Some(300));
    }

    #[test]
    fn test_sort_by_name_case_insensitive() {
        use crate::types::{PortEntry, Protocol, SocketState};
        use std::net::{IpAddr, Ipv4Addr};

        fn make_entry(port: u16, name: &str) -> PortEntry {
            PortEntry { port, protocol: Protocol::Tcp, state: SocketState::Listen,
                        pid: None, process_name: Some(name.to_string()), user: None,
                        local_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED), remote_addr: None, docker_container: None }
        }

        let mut entries = vec![make_entry(80, "Zebra"), make_entry(443, "apple"), make_entry(8080, "Mango")];
        apply_sort(&mut entries, &SortField::Name);
        assert_eq!(entries[0].process_name.as_deref(), Some("apple"));
        assert_eq!(entries[1].process_name.as_deref(), Some("Mango"));
        assert_eq!(entries[2].process_name.as_deref(), Some("Zebra"));
    }
}
