// Rust guideline compliant 2026-02-16

//! Process termination logic.
//!
//! Will support SIGTERM/SIGKILL on Unix and `taskkill` on Windows.
//! Includes safety checks: never kills PID 1, kernel threads,
//! or the current shell process.
