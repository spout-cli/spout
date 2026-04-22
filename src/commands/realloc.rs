//! `spout realloc` — shortcut for `rm` + `alloc` on the same service.
//!
//! Preserves the existing entry's protocol (no `--udp` flag; users
//! who want to switch protocol do `rm` + `alloc` manually). The
//! whole transaction is one `with_lock`: if the fresh allocation
//! can't find a free port, the removal is rolled back and the
//! registry stays intact.

use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::registry;

pub fn run(
    registry_path: &Path,
    service: &str,
    project_override: Option<&str>,
) -> Result<u16, SpoutError> {
    let project = match project_override {
        Some(p) => p.to_owned(),
        None => project::current_project()?,
    };
    registry::with_lock(registry_path, |r| {
        let protocol = r
            .projects
            .get(&project)
            .and_then(|s| s.get(service))
            .map(|e| e.protocol)
            .ok_or(SpoutError::ServiceNotRegistered)?;
        r.remove(&project, service, "reallocated")
            .ok_or(SpoutError::ServiceNotRegistered)?;
        allocator::alloc_within_lock(r, &project, service, protocol).map(|(port, _)| port)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn temp_registry() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn realloc_returns_a_port_in_range_and_keeps_the_entry_live() {
        let (_dir, path) = temp_registry();
        let proj = project::current_project().unwrap();
        registry::with_lock(&path, |r| {
            r.set(&proj, "postgres", 20_000, Protocol::default());
            Ok(())
        })
        .unwrap();
        let port = run(&path, "postgres", None).unwrap();
        assert!((20_000..=32_767).contains(&port));
        // The entry still exists — remove + alloc is atomic under the lock.
        assert_eq!(registry::read(&path).unwrap().get(&proj, "postgres"), Some(port));
    }

    #[test]
    fn realloc_preserves_protocol() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("proj", "dns", 20_053, Protocol::Udp);
            Ok(())
        })
        .unwrap();
        run(&path, "dns", Some("proj")).unwrap();
        let reg = registry::read(&path).unwrap();
        let entry = reg.projects.get("proj").unwrap().get("dns").unwrap();
        assert_eq!(entry.protocol, Protocol::Udp);
    }

    #[test]
    fn realloc_records_reallocated_reason_in_history() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("proj", "postgres", 20_000, Protocol::default());
            Ok(())
        })
        .unwrap();
        run(&path, "postgres", Some("proj")).unwrap();
        let reg = registry::read(&path).unwrap();
        assert_eq!(reg.history.len(), 1);
        assert_eq!(reg.history[0].reason, "reallocated");
        assert_eq!(reg.history[0].port, 20_000);
    }

    #[test]
    fn realloc_unknown_service_errors_one() {
        let (_dir, path) = temp_registry();
        let err = run(&path, "nope", Some("no-proj")).unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn realloc_cross_project_leaves_other_projects_alone() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("a", "postgres", 20_000, Protocol::default());
            r.set("b", "postgres", 20_001, Protocol::default());
            Ok(())
        })
        .unwrap();
        run(&path, "postgres", Some("a")).unwrap();
        let reg = registry::read(&path).unwrap();
        assert_eq!(reg.get("b", "postgres"), Some(20_001));
    }
}
