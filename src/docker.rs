// Rust guideline compliant 2026-02-16

//! Docker container name resolution.
//!
//! Uses the `bollard` crate to connect to the Docker socket and
//! match PIDs to container names. Requires the `docker` feature flag.
//! If Docker is unavailable, all operations degrade gracefully to no-ops.
