// Rust guideline compliant 2026-02-16

//! Docker container name resolution.
//!
//! Uses the `bollard` crate to connect to the Docker socket and
//! match container names to exposed ports. Requires the `docker` feature flag.
//! If Docker is unavailable, all operations degrade gracefully to no-ops.

use std::collections::HashMap;

use crate::types::PortEntry;

/// Enrich port entries with Docker container names by matching on public port numbers.
///
/// When the `docker` feature is enabled, this function connects to the Docker
/// socket, lists all running containers, and populates `docker_container` on
/// any `PortEntry` whose port matches a container's published port.
///
/// All Docker errors are silently swallowed — if Docker is unavailable or the
/// query fails for any reason, the entries are left unchanged.
pub fn enrich_with_docker(entries: &mut [PortEntry]) {
    #[cfg(feature = "docker")]
    {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(enrich_async(entries));
    }

    // Suppress unused-variable warning when feature is disabled.
    #[cfg(not(feature = "docker"))]
    let _ = entries;
}

/// Asynchronously query Docker and apply container name enrichment.
///
/// Connects via the platform default socket path, pings to verify connectivity,
/// lists all running containers, and builds a port → container name map before
/// applying it to the provided entries.
#[cfg(feature = "docker")]
async fn enrich_async(entries: &mut [PortEntry]) {
    use bollard::Docker;
    use bollard::query_parameters::ListContainersOptions;

    // Connect — if this fails (e.g. Docker not installed) we bail silently.
    let docker = match Docker::connect_with_socket_defaults() {
        Ok(d) => d,
        Err(_) => return,
    };

    // Ping to confirm the daemon is reachable.
    if docker.ping().await.is_err() {
        return;
    }

    let options = ListContainersOptions {
        all: false,
        ..Default::default()
    };

    let containers = match docker.list_containers(Some(options)).await {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut port_to_container: HashMap<u16, String> = HashMap::new();

    for container in containers {
        let name = container
            .names
            .as_ref()
            .and_then(|n| n.first())
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_default();

        if name.is_empty() {
            continue;
        }

        if let Some(ports) = container.ports {
            for port in ports {
                if let Some(pub_port) = port.public_port {
                    // First container found wins on port collision; later entries are ignored.
                    port_to_container.entry(pub_port).or_insert(name.clone());
                }
            }
        }
    }

    apply_port_mapping(entries, &port_to_container);
}

/// Apply a port → container name mapping to a slice of `PortEntry` values.
///
/// For each entry whose `port` exists in `port_to_container`, the
/// `docker_container` field is set to the corresponding container name.
/// Entries with ports not present in the map are left unchanged.
#[cfg_attr(not(feature = "docker"), allow(dead_code))]
fn apply_port_mapping(entries: &mut [PortEntry], port_to_container: &HashMap<u16, String>) {
    for entry in entries.iter_mut() {
        if let Some(name) = port_to_container.get(&entry.port) {
            entry.docker_container = Some(name.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::types::{Protocol, SocketState};

    fn make_entry(port: u16) -> PortEntry {
        PortEntry {
            port,
            protocol: Protocol::Tcp,
            state: SocketState::Listen,
            pid: None,
            process_name: None,
            user: None,
            local_addr: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            remote_addr: None,
            docker_container: None,
        }
    }

    #[test]
    fn test_apply_port_mapping_populates_matching_entry() {
        let mut entries = vec![make_entry(5432), make_entry(6379)];
        let mut map = HashMap::new();
        map.insert(5432u16, "postgres".to_string());

        apply_port_mapping(&mut entries, &map);

        assert_eq!(entries[0].docker_container, Some("postgres".to_string()));
        assert_eq!(entries[1].docker_container, None);
    }

    #[test]
    fn test_apply_port_mapping_no_matches_leaves_entries_unchanged() {
        let mut entries = vec![make_entry(8080), make_entry(9090)];
        let map: HashMap<u16, String> = HashMap::new();

        apply_port_mapping(&mut entries, &map);

        assert!(entries.iter().all(|e| e.docker_container.is_none()));
    }

    #[test]
    fn test_apply_port_mapping_multiple_containers() {
        let mut entries = vec![make_entry(80), make_entry(443), make_entry(3306)];
        let mut map = HashMap::new();
        map.insert(80u16, "nginx".to_string());
        map.insert(443u16, "nginx".to_string());
        map.insert(3306u16, "mysql".to_string());

        apply_port_mapping(&mut entries, &map);

        assert_eq!(entries[0].docker_container, Some("nginx".to_string()));
        assert_eq!(entries[1].docker_container, Some("nginx".to_string()));
        assert_eq!(entries[2].docker_container, Some("mysql".to_string()));
    }

    #[test]
    fn test_apply_port_mapping_empty_entries() {
        let mut entries: Vec<PortEntry> = vec![];
        let mut map = HashMap::new();
        map.insert(8080u16, "app".to_string());

        apply_port_mapping(&mut entries, &map);

        assert!(entries.is_empty());
    }
}
