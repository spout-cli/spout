//! Registry schema — data types and their methods. On-disk I/O in `io.rs`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::date::today_iso;
use crate::protocol::Protocol;

pub mod io;
// Re-export what production code uses via the short path
// `crate::registry::*`. `write` and `lock_path` are test-only callers
// (via `with_lock` for prod); they stay reachable at `registry::io::*`
// but aren't re-exported — clippy's `unused_imports` flags a `pub use`
// whose only downstream consumer is a `#[cfg(test)]` module.
pub use io::{read, registry_path, with_lock};

pub const CURRENT_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub projects: HashMap<String, HashMap<String, Entry>>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Entry {
    pub port: u16,
    pub allocated: String,
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HistoryEntry {
    pub project: String,
    pub service: String,
    pub port: u16,
    pub allocated: String,
    pub released: String,
    pub reason: String,
    #[serde(default)]
    pub protocol: Protocol,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            projects: HashMap::new(),
            history: Vec::new(),
        }
    }
}

impl Registry {
    pub fn get(&self, project: &str, service: &str) -> Option<u16> {
        self.projects.get(project)?.get(service).map(|e| e.port)
    }

    pub fn set(&mut self, project: &str, service: &str, port: u16, protocol: Protocol) {
        self.projects.entry(project.to_owned()).or_default().insert(
            service.to_owned(),
            Entry {
                port,
                allocated: today_iso(),
                protocol,
            },
        );
    }

    /// Remove a registration. Appends to history with the given reason.
    /// Returns the port that was removed, or None if not registered.
    pub fn remove(&mut self, project: &str, service: &str, reason: &str) -> Option<u16> {
        let entry = self.projects.get_mut(project)?.remove(service)?;
        if self.projects.get(project).is_some_and(|p| p.is_empty()) {
            self.projects.remove(project);
        }
        let port = entry.port;
        self.history
            .push(history_of(project, service, entry, reason, &today_iso()));
        Some(port)
    }

    /// Remove every service registered under `project`, appending each to
    /// `history` with the given reason. Returns the count removed; 0 if the
    /// project didn't exist. Used by `spout rm --project` so the whole-
    /// project teardown happens in a single mutation pass.
    pub fn remove_project(&mut self, project: &str, reason: &str) -> usize {
        let Some(services) = self.projects.remove(project) else {
            return 0;
        };
        let count = services.len();
        let released = today_iso();
        for (service, entry) in services {
            self.history
                .push(history_of(project, &service, entry, reason, &released));
        }
        count
    }

    /// Live-registry ownership of (port, protocol). TCP and UDP at the same
    /// number are independent — one does not block the other.
    pub fn is_port_claimed(&self, port: u16, protocol: Protocol) -> Option<(String, String)> {
        self.projects.iter().find_map(|(project, services)| {
            services
                .iter()
                .find(|(_, e)| e.port == port && e.protocol == protocol)
                .map(|(service, _)| (project.clone(), service.clone()))
        })
    }

    /// History lookup for a port. Most-recent release first.
    pub fn history_for_port(&self, port: u16) -> Vec<&HistoryEntry> {
        let mut matches: Vec<_> = self.history.iter().filter(|e| e.port == port).collect();
        matches.sort_by(|a, b| b.released.cmp(&a.released));
        matches
    }
}

fn history_of(
    project: &str,
    service: &str,
    entry: Entry,
    reason: &str,
    released: &str,
) -> HistoryEntry {
    HistoryEntry {
        project: project.to_owned(),
        service: service.to_owned(),
        port: entry.port,
        allocated: entry.allocated,
        released: released.to_owned(),
        reason: reason.to_owned(),
        protocol: entry.protocol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_set_get_remove_roundtrip() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456, Protocol::default());
        assert_eq!(r.get("myproj", "postgres"), Some(19456));

        let removed = r.remove("myproj", "postgres", "test");
        assert_eq!(removed, Some(19456));
        assert_eq!(r.get("myproj", "postgres"), None);
        assert_eq!(r.history.len(), 1);
        assert_eq!(r.history[0].port, 19456);
        assert_eq!(r.history[0].reason, "test");
    }

    #[test]
    fn remove_carries_allocated_date_into_history() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456, Protocol::default());
        let live_allocated = r
            .projects
            .get("myproj")
            .unwrap()
            .get("postgres")
            .unwrap()
            .allocated
            .clone();
        r.remove("myproj", "postgres", "test");
        assert_eq!(r.history[0].allocated, live_allocated);
    }

    #[test]
    fn remove_empties_project_entry() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456, Protocol::default());
        r.remove("myproj", "postgres", "test");
        assert!(!r.projects.contains_key("myproj"));
    }

    #[test]
    fn is_port_claimed_finds_existing() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456, Protocol::default());
        let owner = r.is_port_claimed(19456, Protocol::Tcp).unwrap();
        assert_eq!(owner, ("myproj".to_owned(), "postgres".to_owned()));
    }

    #[test]
    fn is_port_claimed_returns_none_for_free() {
        let r = Registry::default();
        assert!(r.is_port_claimed(19456, Protocol::Tcp).is_none());
    }

    #[test]
    fn history_for_port_sorted_most_recent_first() {
        let mut r = Registry::default();
        r.history.push(HistoryEntry {
            project: "a".into(),
            service: "s".into(),
            port: 19456,
            allocated: "2025-09-01".into(),
            released: "2026-01-01".into(),
            reason: "x".into(),
            protocol: Protocol::default(),
        });
        r.history.push(HistoryEntry {
            project: "b".into(),
            service: "s".into(),
            port: 19456,
            allocated: "2026-02-01".into(),
            released: "2026-06-01".into(),
            reason: "y".into(),
            protocol: Protocol::default(),
        });
        let entries = r.history_for_port(19456);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].released, "2026-06-01");
        assert_eq!(entries[1].released, "2026-01-01");
    }
}
