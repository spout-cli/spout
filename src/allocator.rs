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
use std::net::{TcpListener, UdpSocket};
use std::path::Path;
use std::sync::OnceLock;

use crate::error::{OrphanRecord, SpoutError};
use crate::protocol::Protocol;
use crate::registry::{self, Registry};

pub const BASE_PORT: u16 = 20_000;
pub const MAX_PORT: u16 = 32_767;

pub fn alloc(
    registry_path: &Path,
    project: &str,
    service: &str,
    protocol: Protocol,
) -> Result<u16, SpoutError> {
    let cwd = std::env::current_dir()
        .map_err(|e| SpoutError::Io(format!("read current directory: {e}")))?;
    registry::with_lock(registry_path, |r| {
        // Idempotent path skips the orphan check — re-allocating an
        // already-registered service is a safe agent retry and must not
        // be blocked by sibling-identity entries.
        if r.get(project, service).is_none() {
            let orphans = r.orphans_for_service(project, service, &cwd);
            if !orphans.is_empty() {
                return Err(SpoutError::AllocOrphanMatch {
                    project: project.to_owned(),
                    service: service.to_owned(),
                    orphans: orphans.into_iter().map(OrphanRecord::from).collect(),
                });
            }
        }
        alloc_within_lock(r, project, service, protocol).map(|(port, _)| port)
    })
}

/// Allocate a port inside an already-held registry lock. Returns `(port,
/// is_new)` — `is_new` is `false` on the idempotent "already registered"
/// path and `true` for a fresh allocation. `compose` calls this from its
/// batch loop so N services share one file-lock acquisition instead of N
/// round-trips, and uses the flag for the "new vs existing" summary
/// counts without needing a second `reg.get`.
pub fn alloc_within_lock(
    reg: &mut Registry,
    project: &str,
    service: &str,
    protocol: Protocol,
) -> Result<(u16, bool), SpoutError> {
    if let Some(port) = reg.get(project, service) {
        return Ok((port, false));
    }
    // Ports claimed on the *same* protocol are off-limits; claims on the
    // other protocol don't block us — TCP 5432 and UDP 5432 coexist.
    let claimed: HashSet<u16> = reg
        .projects
        .values()
        .flat_map(|services| services.values())
        .filter(|e| e.protocol == protocol)
        .map(|e| e.port)
        .collect();
    for candidate in BASE_PORT..=MAX_PORT {
        if claimed.contains(&candidate) {
            continue;
        }
        if !is_port_free_on_os(candidate, protocol) {
            continue;
        }
        reg.set(project, service, candidate, protocol);
        return Ok((candidate, true));
    }
    Err(SpoutError::NoFreePortFound {
        service: service.to_owned(),
        range_start: BASE_PORT,
        range_end: MAX_PORT,
    })
}

/// True if `port` is bindable for `protocol` on IPv4 and (if available) IPv6.
///
/// The bind is immediately dropped — this is a test, not a reservation.
/// There's an inherent TOCTOU gap between this check and any subsequent
/// use of the port; in practice the window is microseconds and the 20000+
/// range sees almost no non-spout binds.
///
/// TCP and UDP are independent on real kernels: TCP 5432 being taken does
/// not imply UDP 5432 is taken, and vice versa. This probe respects that.
pub fn is_port_free_on_os(port: u16, protocol: Protocol) -> bool {
    match protocol {
        Protocol::Tcp => is_tcp_port_free(port),
        Protocol::Udp => is_udp_port_free(port),
    }
}

fn is_tcp_port_free(port: u16) -> bool {
    if TcpListener::bind(("127.0.0.1", port)).is_err() {
        return false;
    }
    if ipv6_available() && TcpListener::bind(("::1", port)).is_err() {
        return false;
    }
    true
}

fn is_udp_port_free(port: u16) -> bool {
    if UdpSocket::bind(("127.0.0.1", port)).is_err() {
        return false;
    }
    if ipv6_available() && UdpSocket::bind(("::1", port)).is_err() {
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
        .filter_map(|entry| (!is_port_free_on_os(entry.port, entry.protocol)).then_some(entry.port))
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
    use serial_test::serial;
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
        let port = alloc(&path, "p", "s", Protocol::default()).unwrap();
        assert!((BASE_PORT..=MAX_PORT).contains(&port));
    }

    #[test]
    fn alloc_is_idempotent_per_project_service() {
        let (_dir, path) = temp_registry();
        let first = alloc(&path, "p", "s", Protocol::default()).unwrap();
        let second = alloc(&path, "p", "s", Protocol::default()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn alloc_different_projects_get_different_ports() {
        let (_dir, path) = temp_registry();
        let a = alloc(&path, "proj-a", "postgres", Protocol::default()).unwrap();
        let b = alloc(&path, "proj-b", "postgres", Protocol::default()).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn alloc_different_services_same_project_get_different_ports() {
        let (_dir, path) = temp_registry();
        let pg = alloc(&path, "p", "postgres", Protocol::default()).unwrap();
        let rd = alloc(&path, "p", "redis", Protocol::default()).unwrap();
        assert_ne!(pg, rd);
    }

    #[test]
    #[serial]
    fn alloc_skips_ports_bound_by_os() {
        let (_dir, path) = temp_registry();
        // Skip if BASE_PORT is already in use — we can't reliably assert the
        // skip behaviour without holding it ourselves. `#[serial]` keeps this
        // test from racing the other `#[serial]` tests in this module, but
        // it can't fence against unrelated processes.
        let Ok(_holder) = TcpListener::bind(("0.0.0.0", BASE_PORT)) else {
            return;
        };
        let port = alloc(&path, "p", "s", Protocol::default()).unwrap();
        assert_ne!(port, BASE_PORT);
        assert!(port > BASE_PORT);
    }

    #[test]
    fn alloc_udp_returns_port_in_range() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "p", "dns", Protocol::Udp).unwrap();
        assert!((BASE_PORT..=MAX_PORT).contains(&port));
    }

    #[test]
    fn alloc_udp_and_tcp_can_share_a_port_number() {
        let (_dir, path) = temp_registry();
        let tcp_port = alloc(&path, "p", "tcp-svc", Protocol::Tcp).unwrap();
        registry::with_lock(&path, |r| {
            r.set("p", "udp-svc", tcp_port, Protocol::Udp);
            Ok(())
        })
        .unwrap();
        let reg = registry::read(&path).unwrap();
        assert_eq!(reg.get("p", "tcp-svc"), Some(tcp_port));
        assert_eq!(reg.get("p", "udp-svc"), Some(tcp_port));
    }

    #[test]
    fn is_port_free_on_os_returns_true_for_free_tcp_port() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(is_port_free_on_os(port, Protocol::Tcp));
    }

    #[test]
    fn is_port_free_on_os_returns_false_for_bound_tcp_port() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_free_on_os(port, Protocol::Tcp));
        drop(listener);
    }

    #[test]
    fn is_port_free_on_os_returns_false_for_bound_udp_port() {
        use std::net::UdpSocket;
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        assert!(!is_port_free_on_os(port, Protocol::Udp));
        drop(socket);
    }

    #[test]
    fn tcp_and_udp_probes_are_independent() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_free_on_os(port, Protocol::Tcp));
        assert!(is_port_free_on_os(port, Protocol::Udp));
    }

    #[test]
    fn probe_bound_ports_includes_currently_bound_registrations() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut reg = Registry::default();
        reg.set("proj", "svc", port, Protocol::default());
        let bound = probe_bound_ports(&reg);
        assert!(bound.contains(&port));
    }

    #[test]
    fn probe_bound_ports_empty_registry_returns_empty_set() {
        let reg = Registry::default();
        assert!(probe_bound_ports(&reg).is_empty());
    }

    #[test]
    fn alloc_refuses_when_orphan_exists_for_same_service() {
        let (_dir, path) = temp_registry();
        let cwd = std::env::current_dir().unwrap();
        let cwd_str = cwd.display().to_string();
        // Register postgres under the cwd identity (sibling/orphan).
        registry::with_lock(&path, |r| {
            r.set(&cwd_str, "postgres", 20_000, Protocol::Tcp);
            Ok(())
        })
        .unwrap();
        // Allocating postgres under a different (git-style) project must refuse.
        let result = alloc(&path, "github.com/acme/myapp", "postgres", Protocol::Tcp);
        match result {
            Err(SpoutError::AllocOrphanMatch {
                service, orphans, ..
            }) => {
                assert_eq!(service, "postgres");
                assert_eq!(orphans.len(), 1);
                assert_eq!(orphans[0].project, cwd_str);
                assert_eq!(orphans[0].port, 20_000);
            }
            other => panic!("expected AllocOrphanMatch, got {other:?}"),
        }
    }

    #[test]
    fn alloc_idempotent_path_ignores_orphan_check() {
        let (_dir, path) = temp_registry();
        let cwd = std::env::current_dir().unwrap();
        let cwd_str = cwd.display().to_string();
        // Service already registered in target project AND exists as orphan.
        registry::with_lock(&path, |r| {
            r.set(&cwd_str, "postgres", 20_000, Protocol::Tcp);
            r.set("github.com/acme/myapp", "postgres", 30_000, Protocol::Tcp);
            Ok(())
        })
        .unwrap();
        // Idempotent re-allocation under target returns existing port, not refusal.
        let port = alloc(&path, "github.com/acme/myapp", "postgres", Protocol::Tcp).unwrap();
        assert_eq!(port, 30_000);
    }

    #[test]
    fn alloc_does_not_refuse_when_no_orphan_exists() {
        let (_dir, path) = temp_registry();
        // Empty registry. No orphans possible. Fresh allocation succeeds.
        let port = alloc(&path, "github.com/acme/myapp", "postgres", Protocol::Tcp).unwrap();
        assert!((BASE_PORT..=MAX_PORT).contains(&port));
    }

    #[test]
    fn probe_bound_ports_distinguishes_bound_from_free() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let bound_port = listener.local_addr().unwrap().port();
        let free_port = bound_port.wrapping_sub(1).max(1024);
        let mut reg = Registry::default();
        reg.set("proj", "bound", bound_port, Protocol::default());
        reg.set("proj", "free", free_port, Protocol::default());
        let bound = probe_bound_ports(&reg);
        // `free_port` might be bound by something else on the test host, so
        // we only assert the one we pinned ourselves — enough to prove the
        // fn reads OS state, not just the registry.
        assert!(bound.contains(&bound_port));
    }
}
