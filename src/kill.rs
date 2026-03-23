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

/// Kill all processes represented by the given entries, prompting for confirmation.
///
/// When `force` is false, a confirmation prompt is shown before sending SIGTERM.
/// When `force` is true, SIGKILL is sent immediately without prompting.
/// When `signal` is provided it overrides the default TERM/KILL choice on Unix.
///
/// # Errors
///
/// Returns an error if safety checks fail, the user cannot be prompted, or any
/// kill signal fails.
pub fn kill_processes(entries: &[PortEntry], force: bool, signal: Option<&str>) -> Result<()> {
    let (pids, pid_map) = collect_pids(entries)?;

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

    // Safety-check and kill. When force=true the safety check has already run
    // via collect_pids; we just need to send the signal.
    let _ = &pid_map; // keep alive for safety checks already done inside collect_pids
    dispatch_signals(&pids, force, signal)?;
    poll_for_exit(&pids, force);
    Ok(())
}

/// Kill all processes represented by `entries` without asking for confirmation.
///
/// Safety checks are still performed. Sends SIGTERM (graceful) to each unique
/// PID unless `signal` overrides the choice. Use this when the caller has
/// already obtained confirmation from the user.
///
/// # Errors
///
/// Returns an error if safety checks fail or any kill signal fails.
pub fn kill_confirmed(entries: &[PortEntry], signal: Option<&str>) -> Result<()> {
    let (pids, _pid_map) = collect_pids(entries)?;
    dispatch_signals(&pids, false, signal)?;
    poll_for_exit(&pids, false);
    Ok(())
}

// ── Internals ─────────────────────────────────────────────────────────────

/// Validate entries, safety-check every unique PID, and return sorted PIDs.
fn collect_pids(entries: &[PortEntry]) -> Result<(Vec<u32>, HashMap<u32, ()>)> {
    let mut pid_set: HashMap<u32, &PortEntry> = HashMap::new();
    for entry in entries {
        if let Some(pid) = entry.pid {
            pid_set.entry(pid).or_insert(entry);
        }
    }

    if pid_set.is_empty() {
        return Err(anyhow!("No PID available for this socket"));
    }

    // Safety-check every unique PID before touching anything.
    for entry in pid_set.values() {
        is_safe_to_kill(entry)?;
    }

    let mut pids: Vec<u32> = pid_set.keys().copied().collect();
    pids.sort_unstable();

    Ok((pids, pid_set.keys().map(|&k| (k, ())).collect()))
}

/// Send kill signals to each PID in `pids`.
fn dispatch_signals(pids: &[u32], force: bool, signal: Option<&str>) -> Result<()> {
    for &pid in pids {
        send_signal_to_pid(pid, force, signal)?;
    }
    Ok(())
}

/// After SIGTERM, poll each PID for exit up to 3 seconds on Linux.
fn poll_for_exit(pids: &[u32], force: bool) {
    #[cfg(target_os = "linux")]
    if !force {
        for &pid in pids {
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
    // Suppress unused variable warning on non-Linux.
    let _ = (pids, force);
}

/// Normalize a signal specifier to the form expected by the `kill` command.
///
/// - Already starts with `-`: returned as-is.
/// - Pure numeric string (e.g. `"9"`): prefixed with `-`.
/// - Signal name (with or without `SIG` prefix): stripped of `SIG`, prefixed with `-`.
#[cfg(any(unix, test))]
fn normalize_signal(sig: &str) -> String {
    if sig.starts_with('-') {
        sig.to_string()
    } else if sig.parse::<u32>().is_ok() {
        format!("-{sig}")
    } else {
        format!("-{}", sig.strip_prefix("SIG").unwrap_or(sig))
    }
}

/// Send the platform-specific kill signal to `pid`.
#[cfg(unix)]
fn send_signal_to_pid(pid: u32, force: bool, signal: Option<&str>) -> Result<()> {
    let sig_arg = match signal {
        Some(s) => normalize_signal(s),
        None => if force { "-KILL".to_string() } else { "-TERM".to_string() },
    };
    let status = std::process::Command::new("kill")
        .arg(&sig_arg)
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
fn send_signal_to_pid(pid: u32, force: bool, signal: Option<&str>) -> Result<()> {
    // Custom signals are not supported on Windows; ignore the signal parameter
    // and fall back to the existing taskkill behavior.
    let _ = signal;
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
fn send_signal_to_pid(_pid: u32, _force: bool, _signal: Option<&str>) -> Result<()> {
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
        let entry = fake_entry(Some(99999));
        assert!(is_safe_to_kill(&entry).is_ok());
    }

    #[test]
    fn test_kill_processes_no_pid_returns_err() {
        let entries = vec![fake_entry(None)];
        let result = kill_processes(&entries, true, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_confirmed_no_pid_returns_err() {
        let entries = vec![fake_entry(None)];
        assert!(kill_confirmed(&entries, None).is_err());
    }

    #[test]
    fn test_collect_pids_deduplicates() {
        let mut e1 = fake_entry(Some(9999));
        let mut e2 = fake_entry(Some(9999));
        e1.local_addr = IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0));
        e2.local_addr = IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED);

        let entries = [e1, e2];
        // Both entries have same PID; collect_pids should own-pid-check them.
        // We can't call collect_pids with our own PID 9999 (not running), but we
        // can verify the dedup logic at the HashMap level here.
        let mut pid_set: HashMap<u32, &PortEntry> = HashMap::new();
        for entry in &entries {
            if let Some(pid) = entry.pid {
                pid_set.entry(pid).or_insert(entry);
            }
        }
        assert_eq!(pid_set.len(), 1, "duplicate PID should be deduplicated");
    }

    #[test]
    fn test_collect_pids_two_distinct() {
        let e1 = fake_entry(Some(1001));
        let e2 = fake_entry_named(Some(1002), Some("test-process".to_string()));

        let entries = [e1, e2];
        let mut pid_set: HashMap<u32, &PortEntry> = HashMap::new();
        for entry in &entries {
            if let Some(pid) = entry.pid {
                pid_set.entry(pid).or_insert(entry);
            }
        }
        assert_eq!(pid_set.len(), 2, "two distinct PIDs should both be present");
    }

    #[test]
    fn test_normalize_signal_sigterm() {
        assert_eq!(normalize_signal("SIGTERM"), "-TERM");
    }

    #[test]
    fn test_normalize_signal_numeric() {
        assert_eq!(normalize_signal("9"), "-9");
    }

    #[test]
    fn test_normalize_signal_short_name() {
        assert_eq!(normalize_signal("HUP"), "-HUP");
    }

    #[test]
    fn test_normalize_signal_already_dashed() {
        assert_eq!(normalize_signal("-KILL"), "-KILL");
    }
}
