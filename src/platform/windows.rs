// Rust guideline compliant 2026-02-16

use anyhow::{bail, Result};

use crate::types::PortEntry;

use super::PlatformProvider;

/// Windows socket provider.
///
/// Will use Win32 `GetExtendedTcpTable` / `GetExtendedUdpTable`
/// from `iphlpapi` in a future release.
pub struct WindowsProvider;

impl PlatformProvider for WindowsProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        bail!("Windows support not yet implemented — run on Linux for full functionality")
    }
}
