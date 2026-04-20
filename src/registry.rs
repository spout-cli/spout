//! Registry — JSON file, file locking, atomic writes, history.
//!
//! The only public mutation path is `with_lock`: acquire advisory lock →
//! read → mutate via closure → atomic write → release. Callers cannot
//! hold the lock themselves.
//!
//! Reads via `read` are lock-free — safe because writes are atomic
//! (tempfile + rename).

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::date::today_iso;
use crate::error::SpoutError;

pub const CURRENT_VERSION: u32 = 1;

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
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HistoryEntry {
    pub project: String,
    pub service: String,
    pub port: u16,
    pub allocated: String,
    pub released: String,
    pub reason: String,
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

    pub fn set(&mut self, project: &str, service: &str, port: u16) {
        self.projects.entry(project.to_owned()).or_default().insert(
            service.to_owned(),
            Entry {
                port,
                allocated: today_iso(),
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
        self.history.push(HistoryEntry {
            project: project.to_owned(),
            service: service.to_owned(),
            port: entry.port,
            allocated: entry.allocated,
            released: today_iso(),
            reason: reason.to_owned(),
        });
        Some(entry.port)
    }

    /// Live-registry port ownership check. Returns (project, service) if claimed.
    pub fn is_port_claimed(&self, port: u16) -> Option<(String, String)> {
        for (project, services) in &self.projects {
            for (service, entry) in services {
                if entry.port == port {
                    return Some((project.clone(), service.clone()));
                }
            }
        }
        None
    }

    /// History lookup for a port. Most-recent release first.
    pub fn history_for_port(&self, port: u16) -> Vec<&HistoryEntry> {
        let mut matches: Vec<_> = self.history.iter().filter(|e| e.port == port).collect();
        matches.sort_by(|a, b| b.released.cmp(&a.released));
        matches
    }
}

pub fn registry_path() -> Result<PathBuf, SpoutError> {
    if let Ok(path) = std::env::var("SPOUT_REGISTRY") {
        return Ok(PathBuf::from(path));
    }
    dirs::home_dir()
        .map(|h| h.join(".spout.json"))
        .ok_or_else(|| SpoutError::RegistryCorrupt("cannot determine home directory".to_owned()))
}

/// Derive the lock file path from the registry path by replacing the extension.
/// `/tmp/foo.json` → `/tmp/foo.lock`. Critical for test isolation — every test
/// using a unique SPOUT_REGISTRY gets a unique lock file, so tests don't contend.
pub fn lock_path(registry: &Path) -> PathBuf {
    let mut p = registry.to_path_buf();
    p.set_extension("lock");
    p
}

pub fn read(path: &Path) -> Result<Registry, SpoutError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let registry: Registry = serde_json::from_str(&contents)
                .map_err(|e| SpoutError::RegistryCorrupt(format!("parse failed: {e}")))?;
            if registry.version != CURRENT_VERSION {
                return Err(SpoutError::RegistryVersionUnknown(registry.version));
            }
            Ok(registry)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(SpoutError::RegistryCorrupt(format!("read failed: {e}"))),
    }
}

pub fn write(path: &Path, registry: &Registry) -> Result<(), SpoutError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("create parent: {e}")))?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("tempfile: {e}")))?;

    let json = serde_json::to_string_pretty(registry)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("serialise: {e}")))?;

    tmp.write_all(json.as_bytes())
        .map_err(|e| SpoutError::RegistryCorrupt(format!("write: {e}")))?;
    tmp.flush()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("flush: {e}")))?;

    tmp.persist(path)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("rename: {e}")))?;
    Ok(())
}

pub fn with_lock<F, T>(registry: &Path, f: F) -> Result<T, SpoutError>
where
    F: FnOnce(&mut Registry) -> Result<T, SpoutError>,
{
    let lock = lock_path(registry);
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| SpoutError::RegistryCorrupt(format!("create lock parent: {e}")))?;
    }

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("open lock: {e}")))?;

    let mut rw = fd_lock::RwLock::new(file);
    let _guard = rw
        .write()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("acquire lock: {e}")))?;

    let mut r = read(registry)?;
    let result = f(&mut r)?;
    write(registry, &r)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_registry() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn read_missing_file_returns_empty_registry() {
        let (_dir, path) = temp_registry();
        let r = read(&path).unwrap();
        assert_eq!(r.version, CURRENT_VERSION);
        assert!(r.projects.is_empty());
        assert!(r.history.is_empty());
    }

    #[test]
    fn read_corrupt_json_exits_three() {
        let (_dir, path) = temp_registry();
        fs::write(&path, "{ not valid json").unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn read_unknown_version_exits_four() {
        let (_dir, path) = temp_registry();
        fs::write(&path, r#"{"version":99,"projects":{}}"#).unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn write_then_read_round_trip() {
        let (_dir, path) = temp_registry();
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456);
        write(&path, &r).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.get("myproj", "postgres"), Some(19456));
    }

    #[test]
    fn registry_set_get_remove_roundtrip() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456);
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
        r.set("myproj", "postgres", 19456);
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
        r.set("myproj", "postgres", 19456);
        r.remove("myproj", "postgres", "test");
        assert!(!r.projects.contains_key("myproj"));
    }

    #[test]
    fn is_port_claimed_finds_existing() {
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456);
        let owner = r.is_port_claimed(19456).unwrap();
        assert_eq!(owner, ("myproj".to_owned(), "postgres".to_owned()));
    }

    #[test]
    fn is_port_claimed_returns_none_for_free() {
        let r = Registry::default();
        assert!(r.is_port_claimed(19456).is_none());
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
        });
        r.history.push(HistoryEntry {
            project: "b".into(),
            service: "s".into(),
            port: 19456,
            allocated: "2026-02-01".into(),
            released: "2026-06-01".into(),
            reason: "y".into(),
        });
        let entries = r.history_for_port(19456);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].released, "2026-06-01");
        assert_eq!(entries[1].released, "2026-01-01");
    }

    #[test]
    fn with_lock_applies_mutation() {
        let (_dir, path) = temp_registry();
        with_lock(&path, |r| {
            r.set("myproj", "postgres", 19456);
            Ok(())
        })
        .unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.get("myproj", "postgres"), Some(19456));
    }

    #[test]
    fn with_lock_propagates_error_from_closure() {
        let (_dir, path) = temp_registry();
        let err = with_lock(&path, |_| Err::<(), _>(SpoutError::ServiceNotRegistered)).unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn registry_path_resolves_to_an_absolute_path() {
        let path = registry_path().unwrap();
        assert!(path.is_absolute(), "got {path:?}");
        assert!(path.file_name().is_some());
    }

    #[test]
    fn lock_path_replaces_extension() {
        assert_eq!(
            lock_path(Path::new("/tmp/foo.json")),
            PathBuf::from("/tmp/foo.lock")
        );
        assert_eq!(
            lock_path(Path::new("/home/user/.spout.json")),
            PathBuf::from("/home/user/.spout.lock")
        );
    }

    #[test]
    fn concurrent_with_lock_serialises_writes() {
        use std::sync::Arc;
        use std::thread;
        let (dir, path) = temp_registry();
        let path = Arc::new(path);
        let _keep = Arc::new(dir); // keep tempdir alive for all threads

        let mut handles = vec![];
        for i in 0..10u16 {
            let path = Arc::clone(&path);
            let keep = Arc::clone(&_keep);
            handles.push(thread::spawn(move || {
                let _ = &keep;
                with_lock(&path, |r| {
                    r.set("myproj", &format!("svc{i}"), 20_000 + i);
                    Ok(())
                })
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let back = read(&path).unwrap();
        let svcs = back.projects.get("myproj").unwrap();
        assert_eq!(svcs.len(), 10);
    }
}
