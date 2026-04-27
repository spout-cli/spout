//! Command handlers. Each function is the business logic for one CLI
//! subcommand. `main.rs` just dispatches to these.

use std::io::IsTerminal;
use std::path::Path;

use crate::allocator;
use crate::error::{RemovedRecord, SpoutError};
use crate::format;
use crate::project;
use crate::protocol::Protocol;
use crate::registry;
use crate::services::env_var_name;

mod alloc;
mod prune;
mod rm;
pub use alloc::{alloc, compose as alloc_compose};
pub use prune::run as prune;
pub use rm::{run as rm, RmOptions, RmTarget};

pub fn get(
    registry_path: &Path,
    service: &str,
    project_override: Option<&str>,
) -> Result<u16, SpoutError> {
    let project = match project_override {
        Some(p) => p.to_owned(),
        None => project::current_project()?,
    };
    let reg = registry::read(registry_path)?;
    match reg.get(&project, service) {
        Some(port) => Ok(port),
        None => Err(not_registered_in_project(&reg, &project, service)),
    }
}

/// Builds the rich `ServiceNotRegisteredInProject` error from a fresh
/// registry view. Shared by `get` and `rm_one` so both surfaces report
/// the project's actual service names instead of just "not registered".
pub(super) fn not_registered_in_project(
    reg: &registry::Registry,
    project: &str,
    service: &str,
) -> SpoutError {
    let available = reg
        .projects
        .get(project)
        .map(|svcs| {
            let mut names: Vec<String> = svcs.keys().cloned().collect();
            names.sort();
            names
        })
        .unwrap_or_default();
    let recently_removed =
        reg.history_for_service(project, service)
            .first()
            .map(|h| RemovedRecord {
                released: h.released.clone(),
                reason: h.reason.clone(),
            });
    SpoutError::ServiceNotRegisteredInProject {
        project: project.to_owned(),
        service: service.to_owned(),
        available,
        recently_removed,
    }
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

/// `project_filter`: `None` shows all, `Some(None)` is the current project,
/// `Some(Some(name))` is a named one. Returns `Ok(None)` when the TUI has
/// rendered and exited (caller should print nothing more).
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

/// `project_filter` semantics match `ls`, except "no filter" also resolves
/// to the current project — env has no all-projects mode (env-var names
/// would collide). Returns `None` when the project has no registrations.
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
mod tests;
