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

    /// Force kill (SIGKILL) without confirmation.
    #[arg(long = "force", short = 'f')]
    force: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let port_filters = parse_port_filters(&cli.ports)?;

    let provider = platform::get_provider();
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

    // Sort by port number
    entries.sort_by_key(|e| e.port);

    // Enrich entries with Docker container names where ports match.
    docker::enrich_with_docker(&mut entries);

    // Handle kill mode
    if cli.kill || cli.force {
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
