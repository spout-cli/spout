//! Port transport protocol — TCP or UDP.
//!
//! Separated from `registry.rs` so the schema module stays under the
//! 400-line cap with room for future growth.

use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_tcp() {
        assert_eq!(Protocol::default(), Protocol::Tcp);
    }

    #[test]
    fn serialises_as_lowercase_string() {
        assert_eq!(serde_json::to_string(&Protocol::Tcp).unwrap(), r#""tcp""#);
        assert_eq!(serde_json::to_string(&Protocol::Udp).unwrap(), r#""udp""#);
    }

    #[test]
    fn deserialises_from_lowercase_string() {
        assert_eq!(
            serde_json::from_str::<Protocol>(r#""tcp""#).unwrap(),
            Protocol::Tcp
        );
        assert_eq!(
            serde_json::from_str::<Protocol>(r#""udp""#).unwrap(),
            Protocol::Udp
        );
    }

    #[test]
    fn rejects_uppercase_and_unknown_values() {
        assert!(serde_json::from_str::<Protocol>(r#""TCP""#).is_err());
        assert!(serde_json::from_str::<Protocol>(r#""sctp""#).is_err());
    }

    // Integration with the registry schema: these tests exercise how
    // Protocol rides along inside Entry/HistoryEntry and how the v1→v2
    // migration behaves. Lives here rather than in registry.rs to keep
    // that module under the 400-line cap.

    use crate::registry::{read, with_lock, write, Entry, Registry};
    use std::fs;
    use tempfile::TempDir;

    fn temp_path() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn v1_entry_reads_with_protocol_defaulted_to_tcp() {
        let (_dir, path) = temp_path();
        fs::write(
            &path,
            r#"{"version":1,"projects":{"p":{"s":{"port":20000,"allocated":"2026-04-20"}}},"history":[]}"#,
        )
        .unwrap();
        let r = read(&path).unwrap();
        let entry = r.projects.get("p").unwrap().get("s").unwrap();
        assert_eq!(entry.protocol, Protocol::Tcp);
    }

    #[test]
    fn v1_file_upgrades_to_v2_on_first_mutating_write() {
        let (_dir, path) = temp_path();
        fs::write(&path, r#"{"version":1,"projects":{},"history":[]}"#).unwrap();
        with_lock(&path, |r| {
            r.set("p", "s", 20001);
            Ok(())
        })
        .unwrap();
        assert_eq!(read(&path).unwrap().version, 2);
    }

    #[test]
    fn is_port_claimed_filters_by_protocol() {
        // A TCP claim on port 5432 must not block a UDP query at the same
        // number — real kernels treat these as independent.
        let mut r = Registry::default();
        r.set("p", "tcp-svc", 5432);
        assert!(r.is_port_claimed(5432, Protocol::Tcp).is_some());
        assert!(r.is_port_claimed(5432, Protocol::Udp).is_none());
    }

    #[test]
    fn is_port_claimed_finds_udp_registration() {
        let mut r = Registry::default();
        r.projects.entry("p".into()).or_default().insert(
            "dns".into(),
            Entry {
                port: 5353,
                allocated: "2026-04-22".into(),
                protocol: Protocol::Udp,
            },
        );
        let owner = r.is_port_claimed(5353, Protocol::Udp).unwrap();
        assert_eq!(owner, ("p".to_owned(), "dns".to_owned()));
    }

    #[test]
    fn v2_round_trips_udp_entry() {
        let (_dir, path) = temp_path();
        let mut r = Registry::default();
        r.projects.entry("p".into()).or_default().insert(
            "dns".into(),
            Entry {
                port: 20053,
                allocated: "2026-04-22".into(),
                protocol: Protocol::Udp,
            },
        );
        write(&path, &r).unwrap();
        let entry = read(&path)
            .unwrap()
            .projects
            .get("p")
            .unwrap()
            .get("dns")
            .unwrap()
            .clone();
        assert_eq!(entry.protocol, Protocol::Udp);
    }
}
