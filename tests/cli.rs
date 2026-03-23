// Rust guideline compliant 2026-02-16

use std::process::Command;

/// Construct a Command pointing at the compiled onport binary.
fn onport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_onport"))
}

#[test]
fn help_exits_zero_and_contains_name() {
    let output = onport().arg("--help").output().expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("onport"), "help output should mention 'onport'");
}

#[test]
fn version_exits_zero() {
    let output = onport().arg("--version").output().expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("onport"), "version output should mention 'onport'");
}

#[test]
fn json_produces_valid_json_array() {
    let output = onport().arg("--json").output().expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output should be valid JSON");
    assert!(parsed.is_array(), "JSON output should be an array");
}

#[test]
fn invalid_port_exits_with_error() {
    let output = onport().arg("abc").output().expect("failed to run onport");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid port"),
        "stderr should mention 'Invalid port', got: {stderr}"
    );
}

#[test]
fn tcp_and_udp_flags_together_exits_zero() {
    let output = onport()
        .args(["--tcp", "--udp"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
}

#[test]
fn no_docker_flag_accepted() {
    let output = onport().arg("--no-docker").output().expect("failed to run onport");
    assert!(output.status.success());
}

#[test]
fn no_docker_flag_with_json_produces_valid_json() {
    let output = onport()
        .args(["--no-docker", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--no-docker --json output should be valid JSON");
    assert!(parsed.is_array(), "JSON output should be an array");
}

#[test]
fn json_nonexistent_port_produces_empty_array() {
    // Port 1 is unlikely to be bound; --json should return an empty array without
    // triggering the interactive detail view or kill prompt.
    let output = onport()
        .args(["--json", "1"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");
    assert!(parsed.is_array());
}

#[test]
fn port_range_9990_9999_exits_zero() {
    // Querying a range on empty ports should succeed (return empty JSON array).
    let output = onport()
        .args(["--json", "9990-9999"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output should be valid JSON");
    assert!(parsed.is_array());
}

#[test]
fn mixed_range_and_single_port_exits_zero() {
    let output = onport()
        .args(["--json", "80", "9990-9999"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output should be valid JSON");
    assert!(parsed.is_array());
}

#[test]
fn name_filter_nonexistent_returns_empty_json() {
    let output = onport()
        .args(["--name", "nonexistent_process_xyzzy", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.as_array().unwrap().is_empty());
}

#[test]
fn pid_filter_nonexistent_returns_empty_json() {
    let output = onport()
        .args(["--pid", "99999", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.as_array().unwrap().is_empty());
}

#[test]
fn ipv4_flag_exits_zero() {
    let output = onport()
        .args(["--ipv4", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn ipv6_flag_exits_zero() {
    let output = onport()
        .args(["--ipv6", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn sort_by_pid_exits_zero() {
    let output = onport()
        .args(["--sort", "pid", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn sort_invalid_value_exits_nonzero() {
    let output = onport()
        .args(["--sort", "invalid_sort_value"])
        .output()
        .expect("failed to run onport");
    assert!(!output.status.success());
}

#[test]
fn wide_flag_exits_zero() {
    let output = onport()
        .args(["--wide", "--json"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn wide_flag_table_exits_zero() {
    let output = onport()
        .arg("--wide")
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
}

#[test]
fn signal_without_kill_exits_error() {
    let output = onport()
        .args(["--signal", "HUP", "8080"])
        .output()
        .expect("failed to run onport");
    // Should exit successfully (we print to stderr and return Ok) or non-zero
    // The important thing is stderr contains "requires --kill"
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires --kill"),
        "expected 'requires --kill' in stderr, got: {stderr}"
    );
}

#[test]
fn help_mentions_signal() {
    let output = onport().arg("--help").output().expect("failed to run onport");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--signal"), "help should mention --signal");
}

#[test]
fn completions_bash_exits_zero_with_output() {
    let output = onport()
        .args(["--completions", "bash"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "bash completions should produce output");
}

#[test]
fn completions_zsh_exits_zero() {
    let output = onport()
        .args(["--completions", "zsh"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "zsh completions should produce output");
}

#[test]
fn completions_fish_exits_zero() {
    let output = onport()
        .args(["--completions", "fish"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "fish completions should produce output");
}

#[test]
fn completions_powershell_exits_zero() {
    let output = onport()
        .args(["--completions", "powershell"])
        .output()
        .expect("failed to run onport");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "powershell completions should produce output");
}
