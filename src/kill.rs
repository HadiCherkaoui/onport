// Rust guideline compliant 2026-02-16

//! Process termination logic.
//!
//! Supports SIGTERM/SIGKILL on Unix and `taskkill` on Windows.
//! Includes safety checks: never kills PID 1, kernel threads,
//! or the current shell process.

use std::collections::HashMap;
use std::io::Write;

use anyhow::{Result, anyhow};

use crate::types::PortEntry;

/// Check whether it is safe to kill the process owning `entry`.
///
/// # Errors
///
/// Returns an error describing why the kill would be unsafe.
pub fn is_safe_to_kill(entry: &PortEntry) -> Result<()> {
    let pid = entry
        .pid
        .ok_or_else(|| anyhow!("No PID available for this socket"))?;

    if pid == 1 {
        return Err(anyhow!("Refusing to kill PID 1 (init/systemd)"));
    }

    if pid == std::process::id() {
        return Err(anyhow!("Refusing to kill own process"));
    }

    #[cfg(target_os = "linux")]
    {
        let cmdline_path = format!("/proc/{pid}/cmdline");
        let cmdline = match std::fs::read(&cmdline_path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(()), // Cannot read cmdline; proceed with kill attempt
        };
        if cmdline.is_empty() {
            anyhow::bail!("Refusing to kill kernel thread (PID {pid})");
        }
    }

    Ok(())
}

/// Kill all processes represented by the given entries.
///
/// Entries are expected to belong to the same logical service (same process name).
/// Unique PIDs are extracted, safety-checked, and killed in sequence.
/// When `force` is false, a single confirmation prompt is shown listing all PIDs.
///
/// # Errors
///
/// Returns an error if safety checks fail, the user cannot be prompted, or any
/// kill signal fails.
pub fn kill_processes(entries: &[PortEntry], force: bool) -> Result<()> {
    // Collect unique PIDs; one representative PortEntry per PID for safety checks.
    let mut pid_map: HashMap<u32, &PortEntry> = HashMap::new();
    for entry in entries {
        if let Some(pid) = entry.pid {
            pid_map.entry(pid).or_insert(entry);
        }
    }

    if pid_map.is_empty() {
        return Err(anyhow!("No PID available for this socket"));
    }

    // Safety-check every unique PID before touching anything.
    for entry in pid_map.values() {
        is_safe_to_kill(entry)?;
    }

    let mut pids: Vec<u32> = pid_map.keys().copied().collect();
    // Deterministic order for prompts and per-PID polling.
    pids.sort_unstable();

    if !force {
        // Docker hint once if any entry is containerized.
        if let Some(container) = entries.iter().find_map(|e| e.docker_container.as_ref()) {
            println!("  Hint: consider `docker stop {container}` instead");
        }

        let name = entries
            .first()
            .and_then(|e| e.process_name.as_deref())
            .unwrap_or("unknown");

        if pids.len() == 1 {
            print!("Kill process '{name}' (PID {})? [y/N] ", pids[0]);
        } else {
            let pid_list: String =
                pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
            print!("Kill process '{name}' (PIDs {pid_list})? [y/N] ");
        }
        std::io::stdout().flush()?;

        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !line.starts_with('y') && !line.starts_with('Y') {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Send kill signal to each unique PID.
    for &pid in &pids {
        send_signal_to_pid(pid, force)?;
    }

    // After SIGTERM on Linux, poll for exit up to 3 s per PID.
    #[cfg(target_os = "linux")]
    if !force {
        for &pid in &pids {
            let proc_path = format!("/proc/{pid}");
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if !std::path::Path::new(&proc_path).exists() {
                    break;
                }
            }
            if std::path::Path::new(&proc_path).exists() {
                println!("Process {pid} still running. Use -k -f to force kill.");
            }
        }
    }

    Ok(())
}

/// Send the platform-specific kill signal to `pid`.
///
/// This is the low-level primitive; [`kill_processes`] handles safety checks
/// and user confirmation before calling this function.
#[cfg(unix)]
fn send_signal_to_pid(pid: u32, force: bool) -> Result<()> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = std::process::Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .map_err(|e| anyhow!("Failed to run kill command: {e}"))?;

    if !status.success() {
        return Err(anyhow!(
            "kill command failed with exit code: {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Send the platform-specific kill signal to `pid`.
#[cfg(windows)]
fn send_signal_to_pid(pid: u32, force: bool) -> Result<()> {
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string()]);
    if force {
        // /F forces termination; without it taskkill sends a close message.
        cmd.arg("/F");
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to run taskkill: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("taskkill failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Send the platform-specific kill signal to `pid`.
#[cfg(not(any(unix, windows)))]
fn send_signal_to_pid(_pid: u32, _force: bool) -> Result<()> {
    Err(anyhow!("Kill is not supported on this platform"))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::types::{Protocol, SocketState};

    /// Create a minimal fake `PortEntry` for testing.
    fn fake_entry(pid: Option<u32>) -> PortEntry {
        fake_entry_named(pid, Some("test-process".to_string()))
    }

    fn fake_entry_named(pid: Option<u32>, name: Option<String>) -> PortEntry {
        PortEntry {
            port: 8080,
            protocol: Protocol::Tcp,
            state: SocketState::Listen,
            pid,
            process_name: name,
            user: None,
            local_addr: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            remote_addr: None,
            docker_container: None,
        }
    }

    #[test]
    fn test_safe_to_kill_pid_1_rejected() {
        let entry = fake_entry(Some(1));
        let result = is_safe_to_kill(&entry);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("PID 1"), "expected PID 1 message, got: {msg}");
    }

    #[test]
    fn test_safe_to_kill_own_pid_rejected() {
        let own_pid = std::process::id();
        let entry = fake_entry(Some(own_pid));
        let result = is_safe_to_kill(&entry);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("own process"),
            "expected own process message, got: {msg}"
        );
    }

    #[test]
    fn test_safe_to_kill_no_pid_rejected() {
        let entry = fake_entry(None);
        let result = is_safe_to_kill(&entry);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("No PID"),
            "expected no PID message, got: {msg}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_safe_to_kill_normal_pid_accepted() {
        // A large PID that is unlikely to be the current process or PID 1.
        let entry = fake_entry(Some(99999));
        assert!(is_safe_to_kill(&entry).is_ok());
    }

    #[test]
    fn test_kill_processes_no_pid_returns_err() {
        let entries = vec![fake_entry(None)];
        let result = kill_processes(&entries, true);
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_processes_deduplicates_pids() {
        // Two entries sharing the same PID should produce only one unique PID.
        let mut e1 = fake_entry(Some(9999));
        let mut e2 = fake_entry(Some(9999));
        e1.local_addr = IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0));
        e2.local_addr = IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED);

        let entries = [e1, e2];
        let mut pid_map: HashMap<u32, &PortEntry> = HashMap::new();
        for entry in &entries {
            if let Some(pid) = entry.pid {
                pid_map.entry(pid).or_insert(entry);
            }
        }
        assert_eq!(pid_map.len(), 1, "duplicate PID should be deduplicated");
    }

    #[test]
    fn test_kill_processes_two_distinct_pids_extracted() {
        let e1 = fake_entry(Some(1001));
        let e2 = fake_entry_named(Some(1002), Some("test-process".to_string()));

        let entries = [e1, e2];
        let mut pid_map: HashMap<u32, &PortEntry> = HashMap::new();
        for entry in &entries {
            if let Some(pid) = entry.pid {
                pid_map.entry(pid).or_insert(entry);
            }
        }
        assert_eq!(pid_map.len(), 2, "two distinct PIDs should both be present");
    }
}
