// Rust guideline compliant 2026-02-16

//! Well-known port-to-service name mapping.

/// Look up the well-known service name for a port number.
///
/// Returns `None` for ports with no standard name in this table.
pub fn lookup(port: u16) -> Option<&'static str> {
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("dns"),
        67 | 68 => Some("dhcp"),
        80 => Some("http"),
        110 => Some("pop3"),
        123 => Some("ntp"),
        135 => Some("msrpc"),
        137..=139 => Some("netbios"),
        143 => Some("imap"),
        443 => Some("https"),
        445 => Some("smb"),
        465 => Some("smtps"),
        587 => Some("submission"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        1433 => Some("mssql"),
        1521 => Some("oracle"),
        2049 => Some("nfs"),
        3306 => Some("mysql"),
        3389 => Some("rdp"),
        5432 => Some("postgres"),
        5672 => Some("amqp"),
        5900 => Some("vnc"),
        6379 => Some("redis"),
        6443 => Some("kube-api"),
        8080 => Some("http-alt"),
        8443 => Some("https-alt"),
        9090 => Some("prometheus"),
        9200 => Some("elastic"),
        9418 => Some("git"),
        11211 => Some("memcached"),
        15672 => Some("rabbitmq"),
        27017 => Some("mongodb"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_known_ports() {
        assert_eq!(lookup(22), Some("ssh"));
        assert_eq!(lookup(80), Some("http"));
        assert_eq!(lookup(443), Some("https"));
        assert_eq!(lookup(5432), Some("postgres"));
        assert_eq!(lookup(6379), Some("redis"));
    }

    #[test]
    fn test_lookup_unknown_port() {
        assert_eq!(lookup(12345), None);
        assert_eq!(lookup(0), None);
    }
}
