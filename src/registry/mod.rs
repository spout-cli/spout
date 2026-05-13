//! Registry schema — data types and their methods. On-disk I/O in `io.rs`.

use std::collections::HashMap;
use std::path::Path;

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

    /// Move every service from project identity `from` to identity `to`.
    /// Returns the count moved on success, or `Err(conflicts)` listing the
    /// services that exist under both — caller resolves before retry.
    /// Each moved entry records a `reprojected to <to>` history line so
    /// `spout whois <port> --history` shows the lineage. The `from` project
    /// entry is removed entirely when empty.
    pub fn reproject(&mut self, from: &str, to: &str) -> Result<usize, Vec<String>> {
        if let (Some(src), Some(tgt)) = (self.projects.get(from), self.projects.get(to)) {
            let mut conflicts: Vec<String> = src
                .keys()
                .filter(|k| tgt.contains_key(*k))
                .cloned()
                .collect();
            if !conflicts.is_empty() {
                conflicts.sort();
                return Err(conflicts);
            }
        }
        let Some(services) = self.projects.remove(from) else {
            return Ok(0);
        };
        let count = services.len();
        let released = today_iso();
        let reason = format!("reprojected to {to}");
        for (service, entry) in services {
            self.history.push(history_of(
                from,
                &service,
                entry.clone(),
                &reason,
                &released,
            ));
            self.projects
                .entry(to.to_owned())
                .or_default()
                .insert(service, entry);
        }
        Ok(count)
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

    /// History lookup for a (project, service) pair. Most-recent release
    /// first. `released` is an ISO-8601 date so lexical sort matches
    /// chronological order — every writer goes through `today_iso()`.
    pub fn history_for_service(&self, project: &str, service: &str) -> Vec<&HistoryEntry> {
        let mut matches: Vec<_> = self
            .history
            .iter()
            .filter(|e| e.project == project && e.service == service)
            .collect();
        matches.sort_by(|a, b| b.released.cmp(&a.released));
        matches
    }

    /// Find live entries for `service` under project identities OTHER than
    /// `current_project` whose identity is an absolute path that equals
    /// the current directory or any of its ancestors. Used to detect
    /// orphans when a project's identity changes (e.g., `git init` after
    /// services were registered under the cwd path). Returns
    /// `(project_identity, &entry)` pairs in cwd-first, ancestor-walk
    /// order.
    pub fn orphans_for_service<'a>(
        &'a self,
        current_project: &str,
        service: &str,
        cwd: &Path,
    ) -> Vec<(String, &'a Entry)> {
        let mut orphans = Vec::new();
        for ancestor in cwd.ancestors() {
            let path_str = ancestor.display().to_string();
            if path_str == current_project {
                continue;
            }
            if let Some(services) = self.projects.get(&path_str) {
                if let Some(entry) = services.get(service) {
                    orphans.push((path_str, entry));
                }
            }
        }
        orphans
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
mod tests;
