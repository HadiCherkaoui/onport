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
