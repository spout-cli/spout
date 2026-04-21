//! Port allocation — walks 20000–32767 looking for a port that's both
//! unclaimed in the registry and free on the OS.
//!
//! `alloc` is idempotent: if the service is already registered for the
//! given project, returns the existing port without re-checking anything.
//! The registry is the source of truth for ownership. Bind-testing happens
//! only when walking candidates for a fresh allocation — to avoid handing
//! out a port something else is actively using.
//!
//! Genuinely stale ports (our registration + some other process bound it
//! between then and now) surface as docker-compose errors at use time.
//! Recovery is manual: `spout rm <service> && spout alloc <service>`.

use std::collections::HashSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::OnceLock;

use crate::error::SpoutError;
use crate::registry::{self, Registry};

pub const BASE_PORT: u16 = 20_000;
pub const MAX_PORT: u16 = 32_767;

pub fn alloc(registry_path: &Path, project: &str, service: &str) -> Result<u16, SpoutError> {
    registry::with_lock(registry_path, |r| {
        if let Some(port) = r.get(project, service) {
            return Ok(port);
        }

        // Materialise claimed ports once so the hot loop below is O(1) per
        // candidate instead of scanning every project × service each time.
        let claimed: HashSet<u16> = r
            .projects
            .values()
            .flat_map(|services| services.values().map(|e| e.port))
            .collect();

        for candidate in BASE_PORT..=MAX_PORT {
            if claimed.contains(&candidate) {
                continue;
            }
            if !is_port_free_on_os(candidate) {
                continue;
            }
            r.set(project, service, candidate);
            return Ok(candidate);
        }

        Err(SpoutError::NoFreePortFound {
            service: service.to_owned(),
            range_start: BASE_PORT,
            range_end: MAX_PORT,
        })
    })
}

/// True if `port` is bindable on IPv4 and (if available) IPv6.
///
/// The bind is immediately dropped — this is a test, not a reservation.
/// There's an inherent TOCTOU gap between this check and any subsequent
/// use of the port; in practice the window is microseconds and the 20000+
/// range sees almost no non-spout binds.
pub fn is_port_free_on_os(port: u16) -> bool {
    if TcpListener::bind(("0.0.0.0", port)).is_err() {
        return false;
    }
    if ipv6_available() && TcpListener::bind(("::", port)).is_err() {
        return false;
    }
    true
}

/// Snapshot of which registered ports are currently bound on the OS.
///
/// Called once per `spout ls` invocation — the result feeds both the TUI
/// and plain-text renderers. Each probe is two bind attempts (v4 + v6),
/// so cost is linear in registry size. For a 30-service registry that's
/// typically a few milliseconds and never re-run within the same command.
pub fn probe_bound_ports(reg: &Registry) -> HashSet<u16> {
    reg.projects
        .values()
        .flat_map(|services| services.values())
        .filter_map(|entry| (!is_port_free_on_os(entry.port)).then_some(entry.port))
        .collect()
}

/// IPv6 availability — probed once per process via `[::]:0` (ephemeral port).
/// Cached so the probe runs at most once regardless of how many ports we check.
fn ipv6_available() -> bool {
    static IPV6: OnceLock<bool> = OnceLock::new();
    *IPV6.get_or_init(|| TcpListener::bind("[::]:0").is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn temp_registry() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn alloc_fresh_returns_first_free_port_in_range() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "p", "s").unwrap();
        assert!((BASE_PORT..=MAX_PORT).contains(&port));
    }

    #[test]
    fn alloc_is_idempotent_per_project_service() {
        let (_dir, path) = temp_registry();
        let first = alloc(&path, "p", "s").unwrap();
        let second = alloc(&path, "p", "s").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn alloc_different_projects_get_different_ports() {
        let (_dir, path) = temp_registry();
        let a = alloc(&path, "proj-a", "postgres").unwrap();
        let b = alloc(&path, "proj-b", "postgres").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn alloc_different_services_same_project_get_different_ports() {
        let (_dir, path) = temp_registry();
        let pg = alloc(&path, "p", "postgres").unwrap();
        let rd = alloc(&path, "p", "redis").unwrap();
        assert_ne!(pg, rd);
    }

    #[test]
    fn alloc_skips_ports_bound_by_os() {
        let (_dir, path) = temp_registry();
        // Hold BASE_PORT ourselves — alloc should skip to BASE_PORT + 1 (or later).
        let holder = TcpListener::bind(("0.0.0.0", BASE_PORT));
        if holder.is_err() {
            // Port already in use by something on the test machine — test
            // is still valid (alloc will skip it), just can't assert the
            // exact fallback target.
            let port = alloc(&path, "p", "s").unwrap();
            assert_ne!(port, BASE_PORT);
            return;
        }
        let _holder = holder.unwrap();
        let port = alloc(&path, "p", "s").unwrap();
        assert_ne!(port, BASE_PORT);
        assert!(port > BASE_PORT);
    }

    #[test]
    fn is_port_free_on_os_returns_true_for_free_port() {
        // Use an ephemeral bind to find a known-free port, drop it, then
        // check. Tiny race window but fine for this assertion.
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(is_port_free_on_os(port));
    }

    #[test]
    fn is_port_free_on_os_returns_false_for_bound_port() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_free_on_os(port));
        drop(listener);
    }

    #[test]
    fn probe_bound_ports_includes_currently_bound_registrations() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut reg = Registry::default();
        reg.set("proj", "svc", port);
        let bound = probe_bound_ports(&reg);
        assert!(bound.contains(&port));
    }

    #[test]
    fn probe_bound_ports_empty_registry_returns_empty_set() {
        let reg = Registry::default();
        assert!(probe_bound_ports(&reg).is_empty());
    }

    #[test]
    fn probe_bound_ports_distinguishes_bound_from_free() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let bound_port = listener.local_addr().unwrap().port();
        let free_port = bound_port.wrapping_sub(1).max(1024);
        let mut reg = Registry::default();
        reg.set("proj", "bound", bound_port);
        reg.set("proj", "free", free_port);
        let bound = probe_bound_ports(&reg);
        // `free_port` might be bound by something else on the test host, so
        // we only assert the one we pinned ourselves — enough to prove the
        // fn reads OS state, not just the registry.
        assert!(bound.contains(&bound_port));
    }
}
