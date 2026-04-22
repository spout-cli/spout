//! Command handlers. Each function is the business logic for one CLI
//! subcommand. `main.rs` just dispatches to these.

use std::io::IsTerminal;
use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::format;
use crate::project;
use crate::protocol::Protocol;
use crate::registry;
use crate::services::env_var_name;

mod alloc;
mod prune;
pub use alloc::alloc;
pub use prune::run as prune;

pub fn get(registry_path: &Path, service: &str) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    let reg = registry::read(registry_path)?;
    reg.get(&project, service)
        .ok_or(SpoutError::ServiceNotRegistered)
}

pub fn set(
    registry_path: &Path,
    service: &str,
    port: u16,
    protocol: Protocol,
) -> Result<(), SpoutError> {
    validate_port(port, protocol)?;
    let project = project::current_project()?;
    registry::with_lock(registry_path, |r| {
        if let Some((owner_project, _)) = r.is_port_claimed(port, protocol) {
            if owner_project != project {
                return Err(SpoutError::PortAlreadyClaimed {
                    port,
                    protocol,
                    project: owner_project,
                });
            }
        }
        if r.get(&project, service) != Some(port) && !allocator::is_port_free_on_os(port, protocol)
        {
            return Err(SpoutError::PortInUse { port, protocol });
        }
        r.set(&project, service, port, protocol);
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

/// List registered services.
///
/// `project_filter` maps directly from the CLI flag shape:
/// - `None` → show all projects
/// - `Some(None)` → filter to the current project
/// - `Some(Some(name))` → filter to the named project
///
/// Returns `Ok(Some(text))` for the plain-text path (piped stdout or
/// `--no-tui`). Returns `Ok(None)` when the TUI has already rendered and
/// exited, in which case the caller should not print anything further.
pub fn ls(
    registry_path: &Path,
    project_filter: Option<Option<String>>,
    no_tui: bool,
) -> Result<Option<String>, SpoutError> {
    let reg = registry::read(registry_path)?;
    let project_name = match project_filter {
        None => None,
        Some(None) => Some(project::current_project()?),
        Some(Some(name)) => Some(name),
    };

    let bound = allocator::probe_bound_ports(&reg);

    if std::io::stdout().is_terminal() && !no_tui {
        crate::tui::render(&reg, project_name.as_deref(), &bound)?;
        return Ok(None);
    }

    let text = match project_name {
        Some(p) => format::project_block(&p, reg.projects.get(&p), &bound),
        None => format::all(&reg, &bound),
    };
    Ok(Some(text))
}

/// Print `KEY=VALUE` port assignments for a project.
///
/// `project_filter` semantics match `ls`, except "no filter" also
/// resolves to the current project (env has no "all projects" mode —
/// env-var names would collide across projects). Returns `None` when
/// the project has no registrations or doesn't exist, so callers can
/// skip printing cleanly.
pub fn env(
    registry_path: &Path,
    project_filter: Option<Option<String>>,
) -> Result<Option<String>, SpoutError> {
    let project = match project_filter {
        Some(Some(name)) => name,
        _ => project::current_project()?,
    };
    let reg = registry::read(registry_path)?;
    let Some(services) = reg.projects.get(&project).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let mut sorted: Vec<_> = services.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    Ok(Some(
        sorted
            .into_iter()
            .map(|(svc, entry)| format!("{}={}", env_var_name(svc), entry.port))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}

pub fn check(port: u16, protocol: Protocol) -> bool {
    allocator::is_port_free_on_os(port, protocol)
}

struct Claim<'a> {
    protocol: Protocol,
    project: &'a str,
    service: &'a str,
    allocated: &'a str,
}

/// Whois result — `Some(message)` on hit, `None` on miss. Multi-match
/// responses are newline-joined; TCP sorts before UDP for stable output.
pub fn whois(
    registry_path: &Path,
    port: u16,
    include_history: bool,
) -> Result<Option<String>, SpoutError> {
    let reg = registry::read(registry_path)?;
    let mut matches: Vec<Claim> = reg
        .projects
        .iter()
        .flat_map(|(proj, svcs)| {
            svcs.iter()
                .filter(|(_, e)| e.port == port)
                .map(move |(svc, e)| Claim {
                    protocol: e.protocol,
                    project: proj,
                    service: svc,
                    allocated: e.allocated.as_str(),
                })
        })
        .collect();
    matches.sort_by(|a, b| {
        (a.protocol, a.project, a.service).cmp(&(b.protocol, b.project, b.service))
    });
    if !matches.is_empty() {
        let lines: Vec<String> = matches
            .into_iter()
            .map(|c| {
                format!(
                    "{port}/{}: {}/{}  (active, allocated {})",
                    c.protocol, c.project, c.service, c.allocated
                )
            })
            .collect();
        return Ok(Some(lines.join("\n")));
    }
    if include_history {
        let entries = reg.history_for_port(port);
        if !entries.is_empty() {
            return Ok(Some(format::history(&entries)));
        }
    }
    Ok(None)
}

fn validate_port(port: u16, protocol: Protocol) -> Result<(), SpoutError> {
    if port < 1024 {
        return Err(SpoutError::PortInUse { port, protocol });
    }
    Ok(())
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
        let allocated = alloc(&path, "postgres", Protocol::default()).unwrap();
        let fetched = get(&path, "postgres").unwrap();
        assert_eq!(allocated, fetched);
    }

    #[test]
    fn set_registers_port_for_current_project() {
        let (_dir, path) = temp_registry();
        set(&path, "web", 25_000, Protocol::default()).unwrap();
        let port = get(&path, "web").unwrap();
        assert_eq!(port, 25_000);
    }

    #[test]
    fn set_rejects_privileged_port() {
        let (_dir, path) = temp_registry();
        let err = set(&path, "web", 80, Protocol::default()).unwrap_err();
        assert_eq!(err.exit_code(), 6);
    }

    #[test]
    fn rm_removes_and_appends_to_history() {
        let (_dir, path) = temp_registry();
        alloc(&path, "postgres", Protocol::default()).unwrap();
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
        let out = ls(&path, None, true).unwrap().unwrap();
        assert!(out.contains("no registrations"));
    }

    #[test]
    fn ls_shows_project_and_services_after_alloc() {
        let (_dir, path) = temp_registry();
        alloc(&path, "postgres", Protocol::default()).unwrap();
        alloc(&path, "redis", Protocol::default()).unwrap();
        let out = ls(&path, None, true).unwrap().unwrap();
        assert!(out.contains("postgres"));
        assert!(out.contains("redis"));
    }

    #[test]
    fn ls_filters_to_named_project() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("alpha", "postgres", 20_000, Protocol::default());
            r.set("beta", "redis", 20_001, Protocol::default());
            Ok(())
        })
        .unwrap();
        let out = ls(&path, Some(Some("alpha".to_owned())), true)
            .unwrap()
            .unwrap();
        assert!(out.contains("alpha"));
        assert!(out.contains("postgres"));
        assert!(!out.contains("redis"));
    }

    #[test]
    fn env_unknown_project_returns_none() {
        let (_dir, path) = temp_registry();
        assert!(env(&path, Some(Some("never-existed".to_owned())))
            .unwrap()
            .is_none());
    }

    #[test]
    fn env_named_project_emits_sorted_key_value_lines() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("myproj", "redis", 20_001, Protocol::default());
            r.set("myproj", "postgres", 20_000, Protocol::default());
            r.set("myproj", "mailpit-smtp", 20_002, Protocol::default());
            Ok(())
        })
        .unwrap();
        let out = env(&path, Some(Some("myproj".to_owned())))
            .unwrap()
            .unwrap();
        assert_eq!(
            out,
            "MAILPIT_SMTP_PORT=20002\nPOSTGRES_PORT=20000\nREDIS_PORT=20001"
        );
    }

    #[test]
    fn env_current_project_after_alloc_contains_the_service() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "postgres", Protocol::default()).unwrap();
        let out = env(&path, None).unwrap().unwrap();
        assert!(out.contains(&format!("POSTGRES_PORT={port}")));
    }

    #[test]
    fn env_named_project_with_no_services_returns_none() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("proj", "svc", 20_000, Protocol::default());
            r.remove("proj", "svc", "test").unwrap();
            Ok(())
        })
        .unwrap();
        assert!(env(&path, Some(Some("proj".to_owned()))).unwrap().is_none());
    }

    #[test]
    fn check_returns_false_when_port_is_bound() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(!check(port, Protocol::default()));
        // `l` drops at end of scope; no race window.
    }

    #[test]
    fn whois_returns_active_registration() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "postgres", Protocol::default()).unwrap();
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
    fn whois_lists_both_protocols_tcp_first() {
        let (_dir, path) = temp_registry();
        registry::with_lock(&path, |r| {
            r.set("p", "tcp-svc", 20_000, Protocol::Tcp);
            r.set("p", "udp-svc", 20_000, Protocol::Udp);
            Ok(())
        })
        .unwrap();
        let out = whois(&path, 20_000, false).unwrap().unwrap();
        let tcp_pos = out.find("20000/tcp").expect("tcp row missing");
        let udp_pos = out.find("20000/udp").expect("udp row missing");
        assert!(tcp_pos < udp_pos, "expected tcp before udp, got:\n{out}");
    }

    #[test]
    fn whois_history_finds_released_ports() {
        let (_dir, path) = temp_registry();
        let port = alloc(&path, "postgres", Protocol::default()).unwrap();
        rm(&path, "postgres").unwrap();
        assert!(whois(&path, port, false).unwrap().is_none()); // not in live
        let hit = whois(&path, port, true).unwrap().unwrap(); // in history
        assert!(hit.contains("postgres"));
        assert!(hit.contains("user requested"));
    }
}
