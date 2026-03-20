// Rust guideline compliant 2026-02-16

pub mod json;
pub mod table;
pub mod watch;

use anyhow::Result;

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
