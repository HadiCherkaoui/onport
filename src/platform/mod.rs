// Rust guideline compliant 2026-02-16

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!(
    "onport does not support this platform. Supported: linux, macos, windows."
);

use anyhow::Result;

use crate::types::PortEntry;

/// Enumerates network sockets on the current platform.
///
/// Each platform module provides an implementation that reads
/// socket information from OS-specific APIs.
pub trait PlatformProvider {
    /// List all network sockets on this machine.
    ///
    /// # Errors
    ///
    /// Returns an error if the platform-specific socket enumeration fails,
    /// for example due to missing permissions or unsupported OS.
    fn list_sockets(&self) -> Result<Vec<PortEntry>>;
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Return the platform provider for the current OS.
///
/// Selects the correct implementation at compile time using
/// `#[cfg(target_os)]` attributes.
pub fn get_provider() -> Box<dyn PlatformProvider> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxProvider)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsProvider)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsProvider)
    }
}
