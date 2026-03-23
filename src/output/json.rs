// Rust guideline compliant 2026-02-16

use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use serde::Serialize;

use crate::types::{PortEntry, Protocol, SocketState};

/// Render port entries as pretty-printed JSON to stdout.
///
/// Each entry is wrapped in a [`JsonEntry`] view that adds a `service` field
/// (well-known port name, or `null`) without modifying the core [`PortEntry`] type.
///
/// # Errors
///
/// Returns an error if serialization or writing to stdout fails.
pub fn render(entries: &[PortEntry]) -> Result<()> {
    let json_entries: Vec<JsonEntry<'_>> = entries
        .iter()
        .map(|e| JsonEntry {
            port: e.port,
            service: crate::services::lookup(e.port),
            protocol: &e.protocol,
            state: &e.state,
            pid: e.pid,
            process_name: &e.process_name,
            user: &e.user,
            local_addr: &e.local_addr,
            remote_addr: &e.remote_addr,
            docker_container: &e.docker_container,
        })
        .collect();
    let json = serde_json::to_string_pretty(&json_entries)?;
    println!("{json}");
    Ok(())
}

/// JSON-serializable view of a [`PortEntry`] augmented with a `service` field.
///
/// Borrows all fields from `PortEntry` to avoid cloning. The `service` field is
/// a static lookup derived from the port number at render time; it is `null` in
/// JSON when the port has no well-known name.
#[derive(Serialize)]
struct JsonEntry<'a> {
    port: u16,
    service: Option<&'static str>,
    protocol: &'a Protocol,
    state: &'a SocketState,
    pid: Option<u32>,
    process_name: &'a Option<String>,
    user: &'a Option<String>,
    local_addr: &'a IpAddr,
    remote_addr: &'a Option<SocketAddr>,
    docker_container: &'a Option<String>,
}
