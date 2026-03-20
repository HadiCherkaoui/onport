// Rust guideline compliant 2026-02-16

use anyhow::{bail, Result};

use crate::types::PortEntry;

use super::PlatformProvider;

/// macOS socket provider.
///
/// Will use `lsof` parsing or `libproc` bindings in a future release.
pub struct MacOsProvider;

impl PlatformProvider for MacOsProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        bail!("macOS support not yet implemented")
    }
}
