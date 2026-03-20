// Rust guideline compliant 2026-02-16

//! Process termination logic.
//!
//! Supports SIGTERM/SIGKILL on Unix.
//! Includes safety checks: never kills PID 1, kernel threads,
//! or the current shell process.

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
        let cmdline = std::fs::read(&cmdline_path)
            .unwrap_or_default();
        if cmdline.is_empty() {
            return Err(anyhow!("Refusing to kill kernel thread (PID {pid})"));
        }
    }

    Ok(())
}

/// Prompt the user for confirmation before killing a process.
#[cfg_attr(not(unix), allow(dead_code))]
///
/// Returns `true` if the user confirms, `false` otherwise.
///
/// # Errors
///
/// Returns an error if reading from stdin fails.
pub fn confirm_kill(entry: &PortEntry) -> Result<bool> {
    let pid = entry.pid.ok_or_else(|| anyhow!("No PID available for this socket"))?;
    let name = entry
        .process_name
        .as_deref()
        .unwrap_or("unknown");

    print!("Kill process `{name}` (PID {pid})? [y/N] ");
    std::io::stdout().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    if entry.docker_container.is_some() {
        println!("  Hint: consider `docker stop {name}` instead");
    }

    Ok(line.starts_with('y') || line.starts_with('Y'))
}

/// Kill the process owning `entry`.
///
/// Sends SIGTERM (or SIGKILL with `force = true`).
/// After SIGTERM, waits up to 3 s for the process to exit.
///
/// # Errors
///
/// Returns an error if the signal command fails or safety checks fail.
pub fn kill_process(entry: &PortEntry, force: bool) -> Result<()> {
    #[cfg(unix)]
    {
        is_safe_to_kill(entry)?;

        let pid = entry
            .pid
            .ok_or_else(|| anyhow!("No PID available for this socket"))?;

        if !force {
            let confirmed = confirm_kill(entry)?;
            if !confirmed {
                println!("Aborted.");
                return Ok(());
            }
        }

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

        // After SIGTERM, poll for process exit for up to 3 seconds.
        #[cfg(target_os = "linux")]
        if !force {
            let proc_path = format!("/proc/{pid}");
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if !std::path::Path::new(&proc_path).exists() {
                    return Ok(());
                }
            }
            println!("Process still running. Use -kf to force kill.");
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = (entry, force);
        Err(anyhow!("Kill is only supported on Unix"))
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::types::{Protocol, SocketState};

    /// Create a minimal fake `PortEntry` for testing.
    fn fake_entry(pid: Option<u32>) -> PortEntry {
        PortEntry {
            port: 8080,
            protocol: Protocol::Tcp,
            state: SocketState::Listen,
            pid,
            process_name: Some("test-process".to_string()),
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
}
