//! Command handlers. Each function is the business logic for one CLI
//! subcommand. `main.rs` just dispatches to these.

use std::collections::HashMap;
use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::registry::{self, Entry, HistoryEntry, Registry};

pub fn get(registry_path: &Path, service: &str) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    let reg = registry::read(registry_path)?;
    reg.get(&project, service)
        .ok_or(SpoutError::ServiceNotRegistered)
}

pub fn alloc(registry_path: &Path, service: &str) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    allocator::alloc(registry_path, &project, service)
}

pub fn set(registry_path: &Path, service: &str, port: u16) -> Result<(), SpoutError> {
    validate_port(port)?;
    let project = project::current_project()?;
    registry::with_lock(registry_path, |r| {
        if let Some((owner_project, _)) = r.is_port_claimed(port) {
            if owner_project != project {
                return Err(SpoutError::PortAlreadyClaimed {
                    port,
                    project: owner_project,
                });
            }
        }
        if r.get(&project, service) != Some(port) && !allocator::is_port_free_on_os(port) {
            return Err(SpoutError::PortInUse(port));
        }
        r.set(&project, service, port);
        Ok(())
    })
}

pub fn rm(registry_path: &Path, service: &str) -> Result<(), SpoutError> {
    let project = project::current_project()?;
    registry::with_lock(registry_path, |r| {
        r.remove(&project, service, "user requested")
            .ok_or(SpoutError::ServiceNotRegistered)?;
        Ok(())
    })
}

pub fn ls(registry_path: &Path, project_only: bool) -> Result<String, SpoutError> {
    let reg = registry::read(registry_path)?;
    if project_only {
        let project = project::current_project()?;
        Ok(format_project_block(&project, reg.projects.get(&project)))
    } else {
        Ok(format_all(&reg))
    }
}

pub fn check(port: u16) -> bool {
    allocator::is_port_free_on_os(port)
}

/// Whois result — `Some(message)` on hit, `None` on miss.
pub fn whois(
    registry_path: &Path,
    port: u16,
    include_history: bool,
) -> Result<Option<String>, SpoutError> {
    let reg = registry::read(registry_path)?;
    if let Some((project, service)) = reg.is_port_claimed(port) {
        let allocated = reg
            .projects
            .get(&project)
            .and_then(|s| s.get(&service))
            .map(|e| e.allocated.as_str())
            .unwrap_or("?");
        return Ok(Some(format!(
            "{port}: {project}/{service}  (active, allocated {allocated})"
        )));
    }
    if include_history {
        let entries = reg.history_for_port(port);
        if !entries.is_empty() {
            return Ok(Some(format_history(&entries)));
        }
    }
    Ok(None)
}

fn validate_port(port: u16) -> Result<(), SpoutError> {
    if port < 1024 {
        return Err(SpoutError::PortInUse(port));
    }
    Ok(())
}

fn format_all(reg: &Registry) -> String {
    if reg.projects.is_empty() {
        return String::from("(no registrations)");
    }
    let mut sorted: Vec<_> = reg.projects.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    sorted
        .into_iter()
        .map(|(project, services)| format_project_block(project, Some(services)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_project_block(project: &str, services: Option<&HashMap<String, Entry>>) -> String {
    let mut out = String::from(project);
    out.push('\n');
    match services {
        Some(s) if !s.is_empty() => {
            let width = s.keys().map(|k| k.len()).max().unwrap_or(0);
            let mut sorted: Vec<_> = s.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            for (svc, entry) in sorted {
                out.push_str(&format!(
                    "  {:<width$}  {}  (since {})\n",
                    svc,
                    entry.port,
                    entry.allocated,
                    width = width
                ));
            }
        }
        _ => out.push_str("  (no registrations)\n"),
    }
    out.trim_end().to_owned()
}

fn format_history(entries: &[&HistoryEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            format!(
                "{}: was {}/{}  (allocated {}, released {} — {})",
                e.port, e.project, e.service, e.allocated, e.released, e.reason
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn temp_registry() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn get_returns_service_not_registered_when_empty() {
        let (_dir, path) = temp_registry();
        let err = get(&path, "postgres").unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn alloc_then_get_returns_same_port() {
        let (_dir, path) = temp_registry();
        let allocated = alloc(&path, "postgres").unwrap();
        let fetched = get(&path, "postgres").unwrap();
        assert_eq!(allocated, fetched);
    }

    #[test]
    fn set_registers_port_for_current_project() {
        let (_dir, path) = temp_registry();
        set(&path, "web", 25_000).unwrap();
        let port = get(&path, "web").unwrap();
        assert_eq!(port, 25_000);
    }

    #[test]
    fn set_rejects_privileged_port() {
        let (_dir, path) = temp_registry();
        let err = set(&path, "web", 80).unwrap_err();
        assert_eq!(err.exit_code(), 6);
    }

    #[test]
    fn rm_removes_and_appends_to_history() {
        let (_dir, path) = temp_registry();
        alloc(&path, "postgres").unwrap();
        rm(&path, "postgres").unwrap();
        assert!(matches!(
            get(&path, "postgres").unwrap_err(),
            SpoutError::ServiceNotRegistered
        ));
        let reg = registry::read(&path).unwrap();
        assert_eq!(reg.history.len(), 1);
        assert_eq!(reg.history[0].reason, "user requested");
    }

    #[test]
    fn rm_unregistered_service_errors() {
        let (_dir, path) = temp_registry();
        let err = rm(&path, "nothing").unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn ls_empty_registry_is_descriptive() {
        let (_dir, path) = temp_registry();
        let out = ls(&path, false).unwrap();
        assert!(out.contains("no registrations"));
    }

    #[test]
    fn ls_shows_project_and_services_after_alloc() {
        let (_dir, path) = temp_registry();
        alloc(&path, "postgres").unwrap();
        alloc(&path, "redis").unwrap();
        let out = ls(&path, false).unwrap();
        assert!(out.contains("postgres"));
        assert!(out.contains("redis"));
    }

    #[test]
    fn check_returns_true_for_free_port() {
        // Bind to ephemeral, read port, drop, check.
        use std::net::TcpListener;
        let l = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(check(port));
    }

    #[test]
    fn whois_returns_active_registration() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "postgres").unwrap();
        let result = whois(&path, port, false).unwrap().unwrap();
        assert!(result.contains("postgres"));
        assert!(result.contains("active"));
    }

    #[test]
    fn whois_returns_none_when_unknown() {
        let (_dir, path) = temp_registry();
        assert!(whois(&path, 30_000, false).unwrap().is_none());
    }

    #[test]
    fn whois_history_finds_released_ports() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "postgres").unwrap();
        rm(&path, "postgres").unwrap();
        assert!(whois(&path, port, false).unwrap().is_none()); // not in live
        let hit = whois(&path, port, true).unwrap().unwrap(); // in history
        assert!(hit.contains("postgres"));
        assert!(hit.contains("user requested"));
    }
}
