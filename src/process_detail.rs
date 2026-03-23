// Rust guideline compliant 2026-02-16

//! On-demand process detail resolution for the single-port detail view.
//!
//! Provides best-effort resolution of extended process information (full
//! command line, start time, open file-descriptor count, process ancestry)
//! on demand for the single-port detail view. All fields are `Option` and
//! may be `None` when the underlying platform API is unavailable or returns
//! no data.

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
        process_tree: build_process_tree_linux(pid),
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

/// Walk the process ancestry chain using `/proc/{pid}/stat` and `/proc/{pid}/comm`.
///
/// Returns a `→`-separated chain from ancestor to the target process,
/// e.g. `"systemd → docker → postgres"`. Stops at PID ≤ 1 or after
/// 32 levels to prevent infinite loops.
#[cfg(target_os = "linux")]
fn build_process_tree_linux(pid: u32) -> Option<String> {
    use std::collections::HashSet;

    let mut chain: Vec<String> = Vec::new();
    let mut current = pid;
    let mut visited: HashSet<u32> = HashSet::new();

    for _ in 0..32 {
        if current <= 1 || !visited.insert(current) {
            break;
        }

        let comm = std::fs::read_to_string(format!("/proc/{current}/comm"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| format!("pid{current}"));
        chain.push(comm);

        // Extract ppid from /proc/{pid}/stat (field after closing paren, index 1).
        let stat = std::fs::read_to_string(format!("/proc/{current}/stat")).ok()?;
        let after_paren = stat.rfind(')')?.checked_add(2)?;
        let rest = stat.get(after_paren..)?;
        let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;

        if ppid <= 1 {
            break;
        }
        current = ppid;
    }

    if chain.len() < 2 {
        return None;
    }

    chain.reverse();
    Some(chain.join(" \u{2192} ")) // → arrow
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
            process_tree: None,
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
        let proc_start = process.start_time();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Some(format_relative_time(now.saturating_sub(proc_start)))
    };

    ProcessDetails {
        cmdline,
        start_time,
        fd_count: fd_count_windows(pid),
        process_tree: build_process_tree_windows(pid),
    }
}

/// Count open handles for a Windows process using `GetProcessHandleCount`.
///
/// Returns `None` if the process cannot be opened or the API call fails.
#[cfg(target_os = "windows")]
fn fd_count_windows(pid: u32) -> Option<usize> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetProcessHandleCount, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess returns a valid HANDLE when the process exists and
    // we have PROCESS_QUERY_LIMITED_INFORMATION access. We close it below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut count: u32 = 0;
    // SAFETY: handle is valid; count is a valid mutable pointer.
    let ok = unsafe { GetProcessHandleCount(handle, &mut count) };
    // SAFETY: handle was opened above; always close even on error.
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.ok()?;
    Some(count as usize)
}

/// Build a process ancestry chain using `sysinfo` on Windows.
///
/// Walks parent PIDs up to 32 levels, stopping when no parent is found.
#[cfg(target_os = "windows")]
fn build_process_tree_windows(pid: u32) -> Option<String> {
    use std::collections::HashSet;
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let mut chain: Vec<String> = Vec::new();
    let mut current = pid;
    let mut visited: HashSet<u32> = HashSet::new();

    for _ in 0..32 {
        if !visited.insert(current) {
            break;
        }

        let mut system = System::new();
        let pid_sysinfo = Pid::from_u32(current);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid_sysinfo]),
            true,
            ProcessRefreshKind::nothing(),
        );

        let Some(process) = system.process(pid_sysinfo) else {
            break;
        };

        let name = process.name().to_string_lossy().into_owned();
        chain.push(name);

        match process.parent() {
            Some(parent_pid) => {
                let parent_u32 = parent_pid.as_u32();
                if parent_u32 <= 1 {
                    break;
                }
                current = parent_u32;
            }
            None => break,
        }
    }

    if chain.len() < 2 {
        return None;
    }

    chain.reverse();
    Some(chain.join(" \u{2192} "))
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
        .args(["-p", &pid.to_string(), "-o", "etime="])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| parse_etime(&s))
        .map(format_relative_time);

    ProcessDetails {
        cmdline,
        start_time,
        fd_count: fd_count_via_lsof(pid),
        process_tree: build_process_tree_posix(pid),
    }
}

/// Count open file descriptors for a process using `lsof -p`.
///
/// Note: `lsof` may take 100–500 ms. This is only called in the
/// single-port detail view, not the main listing, so latency is acceptable.
///
/// Returns `None` if `lsof` is unavailable or returns no output.
#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn fd_count_via_lsof(pid: u32) -> Option<usize> {
    let output = std::process::Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let count = stdout.lines().count().saturating_sub(1); // subtract header line
    if count == 0 { None } else { Some(count) }
}

/// Build a process ancestry chain via `ps` on macOS and FreeBSD.
///
/// Shells out to `ps -p {pid} -o ppid=` and `ps -p {pid} -o comm=` in a
/// loop. Stops when ppid ≤ 1 or after 32 levels.
#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn build_process_tree_posix(pid: u32) -> Option<String> {
    use std::collections::HashSet;

    let mut chain: Vec<String> = Vec::new();
    let mut current = pid;
    let mut visited: HashSet<u32> = HashSet::new();

    for _ in 0..32 {
        if !visited.insert(current) {
            break;
        }

        let comm = std::process::Command::new("ps")
            .args(["-p", &current.to_string(), "-o", "comm="])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("pid{current}"));
        chain.push(comm);

        let ppid_str = std::process::Command::new("ps")
            .args(["-p", &current.to_string(), "-o", "ppid="])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let Some(ppid_s) = ppid_str else { break };
        let ppid: u32 = match ppid_s.parse() {
            Ok(p) => p,
            Err(_) => break,
        };

        if ppid <= 1 {
            break;
        }
        current = ppid;
    }

    if chain.len() < 2 {
        return None;
    }

    chain.reverse();
    Some(chain.join(" \u{2192} "))
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
        process_tree: None,
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

/// Parse a POSIX `etime` string (`[[dd-]hh:]mm:ss`) into elapsed seconds.
///
/// Used to convert `ps -o etime=` output into a seconds count for
/// `format_relative_time`.
#[cfg_attr(not(any(target_os = "macos", target_os = "freebsd")), allow(dead_code))]
fn parse_etime(etime: &str) -> Option<u64> {
    let etime = etime.trim();
    let (days, rest) = if let Some((d, r)) = etime.split_once('-') {
        (d.parse::<u64>().ok()?, r)
    } else {
        (0u64, etime)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds): (u64, u64, u64) = match parts.len() {
        3 => (parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?),
        2 => (0u64, parts[0].parse().ok()?, parts[1].parse().ok()?),
        _ => return None,
    };
    Some(days * 86_400 + hours * 3_600 + minutes * 60 + seconds)
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

    #[test]
    fn test_parse_etime_minutes_seconds() {
        assert_eq!(parse_etime("05:30"), Some(330));
    }

    #[test]
    fn test_parse_etime_hours_minutes_seconds() {
        assert_eq!(parse_etime("02:15:00"), Some(8100));
    }

    #[test]
    fn test_parse_etime_days() {
        assert_eq!(parse_etime("3-02:00:00"), Some(3 * 86_400 + 2 * 3_600));
    }

    #[test]
    fn test_parse_etime_invalid() {
        assert_eq!(parse_etime("not-a-time"), None);
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
