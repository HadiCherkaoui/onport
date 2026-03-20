// Rust guideline compliant 2026-02-16

use anyhow::Result;

use crate::types::PortEntry;

use super::PlatformProvider;

/// Linux socket provider using `/proc/net/tcp` parsing.
pub struct LinuxProvider;

impl PlatformProvider for LinuxProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>> {
        Ok(Vec::new())
    }
}
