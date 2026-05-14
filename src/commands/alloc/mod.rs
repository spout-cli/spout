//! `spout alloc` — single-service allocation (`alloc`) and
//! compose-file-driven batch allocation (`compose`).

use std::path::{Path, PathBuf};

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::project_markers::{COMPOSE_FILENAMES, OVERRIDE_COMPOSE_FILENAMES};
use crate::protocol::Protocol;
use crate::registry;

mod compose;

use compose::ComposeService;

pub fn alloc(registry_path: &Path, service: &str, protocol: Protocol) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    allocator::alloc(registry_path, &project, service, protocol)
}

/// Result of a compose-mode allocation, split so the caller can print
/// `summary` on stdout and `warnings` on stderr in the right order.
pub struct ComposeOutcome {
    pub summary: String,
    pub warnings: Vec<String>,
}

struct Allocation {
    name: String,
    port: u16,
    protocol: Protocol,
    is_new: bool,
}

pub fn compose(
    registry_path: &Path,
    explicit_files: &[PathBuf],
) -> Result<ComposeOutcome, SpoutError> {
    let cwd =
        std::env::current_dir().map_err(|e| SpoutError::Io(format!("cwd unreadable: {e}")))?;
    let files = resolve_compose_files(&cwd, explicit_files)?;
    let (services, mut warnings) = load_chain(&files)?;

    if services.is_empty() {
        return Ok(ComposeOutcome {
            summary: format!(
                "{}: no services with port declarations.",
                display_files(&files)
            ),
            warnings,
        });
    }

    let project = project::current_project()?;
    let allocations = build_allocations(registry_path, &project, &services, &mut warnings)?;
    let summary = format_compose_summary(&files, &allocations);
    Ok(ComposeOutcome { summary, warnings })
}

fn load_chain(files: &[PathBuf]) -> Result<(Vec<ComposeService>, Vec<String>), SpoutError> {
    let Some((first, rest)) = files.split_first() else {
        return Err(SpoutError::ComposeNotFound(
            "no compose files to load".into(),
        ));
    };
    rest.iter()
        .try_fold(read_and_parse(first)?, |(services, mut warnings), file| {
            let (next, next_warnings) = read_and_parse(file)?;
            warnings.extend(next_warnings);
            Ok((compose::merge_services(services, next), warnings))
        })
}

fn read_and_parse(file: &Path) -> Result<(Vec<ComposeService>, Vec<String>), SpoutError> {
    let yaml = std::fs::read_to_string(file)
        .map_err(|e| SpoutError::ComposeInvalid(format!("read {}: {e}", file.display())))?;
    compose::parse(&yaml)
}

fn display_files(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" + ")
}

fn build_allocations(
    registry_path: &Path,
    project: &str,
    services: &[ComposeService],
    warnings: &mut Vec<String>,
) -> Result<Vec<Allocation>, SpoutError> {
    registry::with_lock(registry_path, |r| {
        let mut allocations = Vec::new();
        for service in services {
            let (allocs, warns) = allocate_service(r, project, service)?;
            allocations.extend(allocs);
            warnings.extend(warns);
        }
        Ok(allocations)
    })
}

fn allocate_service(
    r: &mut registry::Registry,
    project: &str,
    service: &ComposeService,
) -> Result<(Vec<Allocation>, Vec<String>), SpoutError> {
    let mut allocations = Vec::new();
    let mut warnings = Vec::new();
    let mut used = std::collections::HashSet::<String>::new();
    for (idx, port) in service.ports.iter().enumerate() {
        let name = if idx == 0 {
            service.name.clone()
        } else {
            format!("{}-{}", service.name, port.container_port)
        };
        if !used.insert(name.clone()) {
            warnings.push(format!(
                "'{}' declares port {} more than once; skipping the duplicate",
                service.name, port.container_port,
            ));
            continue;
        }
        let (allocated, is_new) = allocator::alloc_within_lock(r, project, &name, port.protocol)?;
        allocations.push(Allocation {
            name,
            port: allocated,
            protocol: port.protocol,
            is_new,
        });
    }
    Ok((allocations, warnings))
}

fn resolve_compose_files(cwd: &Path, explicit: &[PathBuf]) -> Result<Vec<PathBuf>, SpoutError> {
    if !explicit.is_empty() {
        if let Some(missing) = explicit.iter().find(|p| !p.is_file()) {
            return Err(SpoutError::ComposeNotFound(format!(
                "compose file not found: {}",
                missing.display()
            )));
        }
        return Ok(explicit.to_vec());
    }
    let base = find_existing(cwd, COMPOSE_FILENAMES);
    let overlay = find_existing(cwd, OVERRIDE_COMPOSE_FILENAMES);
    match (base, overlay) {
        (Some(base), Some(overlay)) => Ok(vec![base, overlay]),
        (Some(base), None) => Ok(vec![base]),
        (None, Some(overlay)) => Err(SpoutError::ComposeNotFound(format!(
            "found override compose file {} but no base; pass -f <PATH> to specify the base",
            overlay.display()
        ))),
        (None, None) => Err(SpoutError::ComposeNotFound(
            "no compose file found (looked for docker-compose.yml / .yaml / compose.yml / .yaml); \
             pass -f <PATH> to override"
                .to_string(),
        )),
    }
}

fn find_existing(cwd: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| cwd.join(name))
        .find(|c| c.is_file())
}

fn format_compose_summary(files: &[PathBuf], allocations: &[Allocation]) -> String {
    let total = allocations.len();
    let new_count = allocations.iter().filter(|a| a.is_new).count();
    let display = display_files(files);
    let header = if new_count == total {
        format!(
            "{display} → {total} port{} allocated.",
            if total == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{display} → {total} ports ({new_count} new, {} existing).",
            total - new_count,
        )
    };
    let width = allocations.iter().map(|a| a.name.len()).max().unwrap_or(0);
    let rows: Vec<String> = allocations
        .iter()
        .map(|a| format!("  {:<width$}  {}  {}", a.name, a.port, a.protocol))
        .collect();
    format!("{header}\n\n{}", rows.join("\n"))
}

#[cfg(test)]
mod tests;
