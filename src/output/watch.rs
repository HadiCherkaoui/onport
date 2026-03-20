// Rust guideline compliant 2026-02-16

//! Watch mode — live-updating port display.
//!
//! Clears the terminal and redraws the port table every two seconds.
//! Press `q` or `Ctrl+C` to exit. New ports are highlighted green;
//! ports that have disappeared are shown in red for one cycle.

use std::collections::HashSet;
use std::io::Write as _;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use owo_colors::OwoColorize;

use crate::docker;
use crate::platform::PlatformProvider;
use crate::types::{Protocol, SocketState};

// ──────────────────────────────────────────────────────────────────────────────
// Terminal guard
// ──────────────────────────────────────────────────────────────────────────────

/// RAII guard that restores terminal state when dropped.
///
/// On creation it switches to the alternate screen buffer, enables raw mode,
/// and hides the cursor. On drop it reverses all of those operations
/// (best-effort; errors are silently ignored so that cleanup always runs).
struct TerminalGuard;

impl TerminalGuard {
    /// Enter the alternate screen and raw mode.
    ///
    /// # Errors
    ///
    /// Returns an error if crossterm cannot configure the terminal.
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::cursor::Hide,
        )?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort cleanup — ignore errors so the destructor never panics.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show,
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────────

/// Run the live-updating watch loop.
///
/// Queries the platform provider every two seconds, applies the same filtering
/// as the normal display path, diffs against the previous snapshot to detect
/// new / gone ports, and redraws the table with color highlights.
///
/// The loop exits when the user presses `q`, `Q`, or `Ctrl+C`.
///
/// # Errors
///
/// Returns an error if socket enumeration fails on the first query, or if
/// writing to the terminal fails.
pub fn run_watch(
    provider: &dyn PlatformProvider,
    port_filters: &[u16],
    protocol_filter: Option<Protocol>,
    show_all_states: bool,
    no_color: bool,
    no_docker: bool,
) -> Result<()> {
    let _guard = TerminalGuard::enter()?;

    // Previous snapshot: (port, local_addr) pairs seen in the last cycle.
    let mut prev_keys: HashSet<(u16, IpAddr)> = HashSet::new();

    loop {
        // ── 1. Collect & filter ──────────────────────────────────────────────
        let mut entries = provider.list_sockets()?;

        if !port_filters.is_empty() {
            entries.retain(|e| port_filters.contains(&e.port));
        }

        match &protocol_filter {
            Some(Protocol::Tcp) => entries.retain(|e| e.protocol == Protocol::Tcp),
            Some(Protocol::Udp) => entries.retain(|e| e.protocol == Protocol::Udp),
            None => {}
        }

        if !show_all_states {
            entries.retain(|e| e.state == SocketState::Listen);
        }

        // Deduplicate wildcard IPv4/IPv6 entries that represent the same socket
        crate::dedup_entries(&mut entries);

        entries.sort_by_key(|e| e.port);

        if !no_docker {
            docker::enrich_with_docker(&mut entries);
        }

        // Collapse same-service entries (e.g., docker-proxy IPv4/IPv6) for display.
        crate::dedup_same_service(&mut entries);

        // ── 2. Diff against previous snapshot ────────────────────────────────
        let curr_keys: HashSet<(u16, IpAddr)> =
            entries.iter().map(|e| (e.port, e.local_addr)).collect();

        let new_keys: HashSet<(u16, IpAddr)> =
            curr_keys.difference(&prev_keys).copied().collect();

        let gone_keys: HashSet<(u16, IpAddr)> =
            prev_keys.difference(&curr_keys).copied().collect();

        // ── 3. Redraw ─────────────────────────────────────────────────────────
        let now = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let hh = (secs / 3600) % 24;
            let mm = (secs / 60) % 60;
            let ss = secs % 60;
            format!("{hh:02}:{mm:02}:{ss:02}")
        };

        let mut out = std::io::stdout();
        crossterm::execute!(
            out,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
        )?;

        // Header
        let header_line = format!(
            "onport watch \u{2014} press q to quit  (last updated: {now})"
        );
        if no_color {
            println!("{header_line}");
        } else {
            println!("{}", header_line.bold());
        }
        println!();

        // Column header
        print_column_header(no_color);

        // Current entries
        for entry in &entries {
            let key = (entry.port, entry.local_addr);
            let highlight = if new_keys.contains(&key) {
                RowHighlight::New
            } else {
                RowHighlight::Normal
            };
            print_row(entry, highlight, no_color);
        }

        // Gone entries (shown for one cycle in red)
        // We don't have the full PortEntry for gone sockets, so we print a
        // minimal placeholder row indicating the port is no longer present.
        let mut gone_sorted: Vec<_> = gone_keys.iter().copied().collect();
        gone_sorted.sort_by_key(|(port, _)| *port);
        for (port, addr) in gone_sorted {
            let addr_str = super::format_address(&addr);
            let row = format!(
                "  {:>5}  {:<4}  {:<16}  {:<16}  {:>6}  {:<10}  {}",
                port, "—", addr_str, "—", "—", "—", "GONE"
            );
            if no_color {
                println!("{row}");
            } else {
                println!("{}", row.red());
            }
        }

        out.flush()?;

        // ── 4. Update snapshot ────────────────────────────────────────────────
        prev_keys = curr_keys;

        // ── 5. Poll for quit key (2-second timeout) ───────────────────────────
        if event::poll(Duration::from_millis(2000))?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') => break,
                KeyCode::Char('c')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    break
                }
                _ => {}
            }
        }
    }

    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Rendering helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Row color category used when rendering a single port entry.
enum RowHighlight {
    /// Entry exists in both current and previous snapshot.
    Normal,
    /// Entry is new since the last refresh cycle.
    New,
}

/// Print the column header line followed by a separator.
fn print_column_header(no_color: bool) {
    let header = format!(
        "  {:<5}  {:<4}  {:<16}  {:<16}  {:<6}  {:<10}  {}",
        "PORT", "PROTO", "ADDRESS", "PROCESS", "PID", "USER", "STATE"
    );
    let sep =
        "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}  \u{2500}\u{2500}\u{2500}\u{2500}  \
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}  \
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}  \
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}  \
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}  \
         \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}";

    if no_color {
        println!("{header}");
        println!("{sep}");
    } else {
        println!("{}", header.bold());
        println!("{sep}");
    }
}

/// Print a single port-entry row, applying highlight color when appropriate.
fn print_row(entry: &crate::types::PortEntry, highlight: RowHighlight, no_color: bool) {
    let process_name = entry.process_name.as_deref().unwrap_or("?");
    let process_display = if process_name.chars().count() > 16 {
        let truncate_at = process_name
            .char_indices()
            .nth(15)
            .map(|(i, _)| i)
            .unwrap_or(process_name.len());
        format!("{}\u{2026}", &process_name[..truncate_at])
    } else {
        process_name.to_string()
    };

    let addr_str = super::format_address(&entry.local_addr);
    let pid_str = entry
        .pid
        .map_or_else(|| "?".to_string(), |p| p.to_string());
    let user_str = entry.user.clone().unwrap_or_else(|| "?".to_string());
    let state_str = entry.state.to_string();

    let docker_suffix = entry
        .docker_container
        .as_ref()
        .map(|name| format!("  [docker: {name}]"))
        .unwrap_or_default();

    let row = format!(
        "  {:>5}  {:<4}  {:<16}  {:<16}  {:>6}  {:<10}  {}{}",
        entry.port,
        entry.protocol,
        addr_str,
        process_display,
        pid_str,
        user_str,
        state_str,
        docker_suffix,
    );

    if no_color {
        println!("{row}");
        return;
    }

    match highlight {
        RowHighlight::New => println!("{}", row.green()),
        RowHighlight::Normal => println!("{row}"),
    }
}

