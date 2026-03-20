// Rust guideline compliant 2026-02-16

use anyhow::Result;

use crate::types::PortEntry;

/// Render port entries as pretty-printed JSON to stdout.
///
/// # Errors
///
/// Returns an error if serialization or writing to stdout fails.
pub fn render(entries: &[PortEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(entries)?;
    println!("{json}");
    Ok(())
}
