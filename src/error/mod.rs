//! `SpoutError` — every failure mode maps to exactly one exit code.
//!
//! Exit codes are part of the CLI's stable API. See README.md.

use thiserror::Error;

use crate::protocol::Protocol;

#[derive(Debug, Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum SpoutError {
    #[error("service not registered")]
    ServiceNotRegistered,

    #[error("{}", format_not_registered_help(.project, .service, .available, .recently_removed.as_ref(), .orphans))]
    ServiceNotRegisteredInProject {
        project: String,
        service: String,
        available: Vec<String>,
        recently_removed: Option<RemovedRecord>,
        orphans: Vec<OrphanRecord>,
    },

    #[error("no free port found for {service} in range {range_start}-{range_end}")]
    NoFreePortFound {
        service: String,
        range_start: u16,
        range_end: u16,
    },

    #[error("registry unreadable: {0}")]
    RegistryCorrupt(String),

    #[error("registry version {0} is not supported")]
    RegistryVersionUnknown(u32),

    #[error("port {port}/{protocol} is already registered to project '{project}'")]
    PortAlreadyClaimed {
        port: u16,
        protocol: Protocol,
        project: String,
    },

    #[error("port {port}/{protocol} is already in use by the operating system")]
    PortInUse { port: u16, protocol: Protocol },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("compose file unreadable: {0}")]
    ComposeInvalid(String),

    #[error("{0}")]
    ComposeNotFound(String),

    #[error("{0}")]
    Usage(String),

    #[error("{}", format_reproject_conflict(.from, .to, .services))]
    ReprojectConflict {
        from: String,
        to: String,
        services: Vec<String>,
    },

    #[error("{}", format_alloc_orphan_match(.project, .service, .orphans))]
    AllocOrphanMatch {
        project: String,
        service: String,
        orphans: Vec<OrphanRecord>,
    },
}

/// Snapshot of a service's most recent removal record. Independent of
/// `registry::HistoryEntry` so `error.rs` doesn't depend on registry
/// types — the mapping happens at the error-construction site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRecord {
    pub released: String,
    pub reason: String,
}

/// One live entry found under a sibling project identity during the
/// orphan scan. Mirrors `RemovedRecord` in keeping `error.rs` free of
/// registry-type dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanRecord {
    pub project: String,
    pub port: u16,
    pub protocol: Protocol,
}

fn format_not_registered_help(
    project: &str,
    service: &str,
    available: &[String],
    recently_removed: Option<&RemovedRecord>,
    orphans: &[OrphanRecord],
) -> String {
    let mut lines = vec![format!("no service '{service}' in project '{project}'")];
    if available.is_empty() {
        lines.push("  no services currently registered".to_string());
    } else {
        lines.push(format!("  available: {}", available.join(", ")));
    }
    if let Some(r) = recently_removed {
        lines.push(format!(
            "  recently removed: {service} ({}, \"{}\")",
            r.released, r.reason
        ));
    }
    for orphan in orphans {
        lines.push(format!(
            "  registered under different identity: {}/{service} → {}/{}",
            orphan.project, orphan.port, orphan.protocol
        ));
    }
    let hint = if let Some(first) = orphans.first() {
        format!(
            "  (try `spout reproject --from {} --to {project}`)",
            first.project
        )
    } else {
        match (available.is_empty(), recently_removed.is_some()) {
            (true, false) => format!("  (try `spout alloc {service}`)"),
            (true, true) => format!("  (try `spout alloc {service}` to register fresh)"),
            (false, _) => "  (try `spout env` for KEY=VALUE)".to_string(),
        }
    };
    lines.push(hint);
    lines.join("\n")
}

impl SpoutError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ServiceNotRegistered => 1,
            Self::ServiceNotRegisteredInProject { .. } => 1,
            Self::NoFreePortFound { .. } => 2,
            Self::RegistryCorrupt(_) => 3,
            Self::RegistryVersionUnknown(_) => 4,
            Self::PortAlreadyClaimed { .. } => 5,
            Self::PortInUse { .. } => 6,
            Self::Io(_) => 7,
            Self::ComposeInvalid(_) => 8,
            Self::ComposeNotFound(_) => 8,
            Self::Usage(_) => 9,
            Self::ReprojectConflict { .. } => 11,
            Self::AllocOrphanMatch { .. } => 10,
        }
    }
}

fn format_reproject_conflict(from: &str, to: &str, services: &[String]) -> String {
    let mut lines = vec![format!("cannot reproject from '{from}' to '{to}'")];
    lines.push(format!("  services exist in both: {}", services.join(", ")));
    lines.push("  resolve by removing one side first, then retry".to_string());
    lines.join("\n")
}

fn format_alloc_orphan_match(project: &str, service: &str, orphans: &[OrphanRecord]) -> String {
    let mut lines = vec![format!(
        "refusing to allocate '{service}' in project '{project}'"
    )];
    if orphans.len() == 1 {
        let o = &orphans[0];
        lines.push(format!(
            "  '{service}' already exists under sibling identity: {} → {}/{}",
            o.project, o.port, o.protocol
        ));
    } else {
        lines.push(format!(
            "  '{service}' already exists under sibling identities:"
        ));
        for o in orphans {
            lines.push(format!("    {} → {}/{}", o.project, o.port, o.protocol));
        }
    }
    if let Some(first) = orphans.first() {
        lines.push(format!(
            "  (try `spout reproject --from {} --to {project}`)",
            first.project
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
