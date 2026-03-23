// Rust guideline compliant 2026-02-16

use anyhow::Result;
use owo_colors::OwoColorize;
use tabled::settings::object::Columns;
use tabled::settings::{Alignment, Style};
use tabled::{Table, Tabled};

use crate::types::{PortEntry, SocketState};

use super::RenderOptions;

/// A single row in the output table.
#[derive(Tabled)]
struct TableRow {
    #[tabled(rename = "PORT")]
    port: String,
    #[tabled(rename = "SERVICE")]
    service: String,
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
    #[tabled(rename = "REMOTE")]
    remote: String,
}

/// Render port entries as a colored, aligned table.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn render(entries: &[PortEntry], options: &RenderOptions) -> Result<()> {
    if entries.is_empty() {
        println!("No matching sockets found.");
        return Ok(());
    }

    let rows: Vec<TableRow> = entries
        .iter()
        .map(|e| {
            let process_name = e.process_name.as_deref().unwrap_or("?");
            // Truncate process names to PROCESS_COL_WIDTH characters for alignment
            // unless wide mode is enabled.
            let process_display = if !options.wide && process_name.chars().count() > super::PROCESS_COL_WIDTH {
                let truncate_at = process_name
                    .char_indices()
                    .nth(super::PROCESS_COL_WIDTH - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(process_name.len());
                format!("{}…", &process_name[..truncate_at])
            } else {
                process_name.to_string()
            };

            let state_str = e.state.to_string();
            let state_display = if options.no_color {
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
                service: crate::services::lookup(e.port).unwrap_or("—").to_string(),
                proto: e.protocol.to_string(),
                address: super::format_address(&e.local_addr),
                process: process_display,
                pid: e.pid.map_or_else(|| "?".to_string(), |p| p.to_string()),
                user: e.user.clone().unwrap_or_else(|| "?".to_string()),
                state: format!("{state_display}{docker_suffix}"),
                remote: e.remote_addr.map(|sa| sa.to_string()).unwrap_or_else(|| "—".to_string()),
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

