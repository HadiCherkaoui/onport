// Rust guideline compliant 2026-02-16

//! On-demand process detail resolution for the single-port detail view.
//!
//! Provides best-effort resolution of extended process information (full
//! command line, start time, open file-descriptor count) on demand for
//! the single-port detail view. All fields are `Option` and may be `None`
//! when the underlying platform API is unavailable or returns no data.

use crate::types::ProcessDetails;

/// Resolve extended details for the process identified by `pid`.
///
/// Returns a [`ProcessDetails`] populated on a best-effort basis.
/// Fields that cannot be determined on the current platform are `None`.
pub fn resolve(pid: u32) -> ProcessDetails {
    resolve_impl(pid)
}

// ── Linux ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn resolve_impl(pid: u32) -> ProcessDetails {
    ProcessDetails {
        cmdline: cmdline_linux(pid),
        start_time: start_time_linux(pid),
        fd_count: fd_count_linux(pid),
    }
}

/// Read the full command line from `/proc/{pid}/cmdline`.
///
/// Returns `None` if the file is unreadable or the process is a kernel thread
/// (empty cmdline).
#[cfg(target_os = "linux")]
fn cmdline_linux(pid: u32) -> Option<String> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if bytes.is_empty() {
        // Kernel threads have an empty cmdline.
        return None;
    }
    // Args are NUL-separated; replace each NUL with a space for display.
    let cmdline: Vec<u8> = bytes.iter().map(|&b| if b == 0 { b' ' } else { b }).collect();
    String::from_utf8(cmdline).ok().map(|s| s.trim().to_string())
}

/// Compute the process start time as a human-readable relative string.
///
/// Reads `/proc/{pid}/stat` for `starttime` (clock ticks since boot) and
/// `/proc/stat` for `btime` (boot epoch). Returns `None` on any parse failure.
#[cfg(target_os = "linux")]
fn start_time_linux(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;

    // The process name field (field 2) is in parentheses and may contain spaces
    // or nested parens. Find the LAST ')' to safely skip past it.
    let after_paren = stat.rfind(')')?.checked_add(2)?;
    let rest = stat.get(after_paren..)?;

    // After the closing paren+space the fields are (0-indexed):
    //   state(0) ppid(1) pgrp(2) session(3) tty_nr(4) tpgid(5) flags(6)
    //   minflt(7) cminflt(8) majflt(9) cmajflt(10) utime(11) stime(12)
    //   cutime(13) cstime(14) priority(15) nice(16) num_threads(17)
    //   itrealvalue(18) starttime(19)
    let starttime: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;

    // CLK_TCK is 100 ticks/s on virtually all modern Linux architectures.
    // This is a stable kernel ABI value documented in proc(5); no syscall needed.
    const CLK_TCK: u64 = 100;

    let proc_stat = std::fs::read_to_string("/proc/stat").ok()?;
    let btime: u64 = proc_stat
        .lines()
        .find(|l| l.starts_with("btime "))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;

    let proc_start_secs = btime + (starttime / CLK_TCK);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Some(format_relative_time(now.saturating_sub(proc_start_secs)))
}

/// Count open file descriptors by listing entries in `/proc/{pid}/fd/`.
///
/// Returns `None` if the directory cannot be read (e.g. insufficient permission).
#[cfg(target_os = "linux")]
fn fd_count_linux(pid: u32) -> Option<usize> {
    let count = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?.count();
    Some(count)
}

// ── Windows ────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn resolve_impl(pid: u32) -> ProcessDetails {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let pid_sysinfo = Pid::from_u32(pid);
    let refresh_kind = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always);

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid_sysinfo]),
        true,
        refresh_kind,
    );

    let Some(process) = system.process(pid_sysinfo) else {
        return ProcessDetails {
            cmdline: None,
            start_time: None,
            fd_count: None,
        };
    };

    let cmdline = {
        let args: Vec<String> = process
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if args.is_empty() { None } else { Some(args.join(" ")) }
    };

    let start_time = {
        let proc_start = process.start_time(); // seconds since Unix epoch
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Some(format_relative_time(now.saturating_sub(proc_start)))
    };

    ProcessDetails {
        cmdline,
        start_time,
        // Open handle count requires additional Win32 API calls; omit for now.
        fd_count: None,
    }
}

// ── macOS / FreeBSD ────────────────────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn resolve_impl(pid: u32) -> ProcessDetails {
    // Use POSIX-standard ps flags (-p, -o) that work on both macOS and FreeBSD.
    let cmdline = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let start_time = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    ProcessDetails {
        cmdline,
        start_time,
        // fd count via ps is not straightforward; graceful degradation.
        fd_count: None,
    }
}

// ── Fallback ────────────────────────────────────────────────────────────────

#[cfg(not(any(
    target_os = "linux",
    target_os = "windows",
    target_os = "macos",
    target_os = "freebsd",
)))]
fn resolve_impl(_pid: u32) -> ProcessDetails {
    ProcessDetails {
        cmdline: None,
        start_time: None,
        fd_count: None,
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// Format an elapsed duration (in seconds) as a human-readable relative string.
///
/// # Examples
///
/// ```
/// // 42s ago, 5m ago, 2h 15m ago, 3d 4h ago
/// ```
fn format_relative_time(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 { format!("{h}h ago") } else { format!("{h}h {m}m ago") }
    } else {
        let d = secs / 86_400;
        let h = (secs % 86_400) / 3600;
        if h == 0 { format!("{d}d ago") } else { format!("{d}d {h}h ago") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_time_seconds() {
        assert_eq!(format_relative_time(42), "42s ago");
        assert_eq!(format_relative_time(0), "0s ago");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        assert_eq!(format_relative_time(90), "1m ago");
        assert_eq!(format_relative_time(300), "5m ago");
        // 59m 59s rounds down to 59m
        assert_eq!(format_relative_time(3599), "59m ago");
    }

    #[test]
    fn test_format_relative_time_hours() {
        assert_eq!(format_relative_time(3600), "1h ago");
        assert_eq!(format_relative_time(3600 * 2 + 60 * 15), "2h 15m ago");
        // Exactly 1h, no minutes
        assert_eq!(format_relative_time(3600 * 5), "5h ago");
    }

    #[test]
    fn test_format_relative_time_days() {
        assert_eq!(format_relative_time(86_400), "1d ago");
        assert_eq!(format_relative_time(86_400 * 3 + 3600 * 4), "3d 4h ago");
        // Exactly 1d, no hours
        assert_eq!(format_relative_time(86_400 * 2), "2d ago");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_resolve_self_has_cmdline() {
        let details = resolve(std::process::id());
        assert!(details.cmdline.is_some(), "should resolve own cmdline on Linux");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_resolve_self_has_cmdline() {
        let details = resolve(std::process::id());
        assert!(details.cmdline.is_some(), "should resolve own cmdline on Windows");
    }
}
