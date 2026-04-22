//! `spout alloc` — single-service allocation (`alloc`) and
//! compose-file-driven batch allocation (`alloc_compose`).

use std::path::{Path, PathBuf};

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::protocol::Protocol;
use crate::registry;

mod compose;

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

const COMPOSE_FILENAMES: &[&str] = &[
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
];

pub fn compose(
    registry_path: &Path,
    explicit_file: Option<&Path>,
) -> Result<ComposeOutcome, SpoutError> {
    let cwd =
        std::env::current_dir().map_err(|e| SpoutError::Io(format!("cwd unreadable: {e}")))?;
    let file = discover_compose(&cwd, explicit_file)?;
    let yaml = std::fs::read_to_string(&file)
        .map_err(|e| SpoutError::ComposeInvalid(format!("read {}: {e}", file.display())))?;
    let services = compose::parse(&yaml)?;

    let mut warnings: Vec<String> = services
        .iter()
        .filter(|s| s.extra_ports > 0)
        .map(|s| {
            format!(
                "'{}' declares {} ports; allocating only the first",
                s.name,
                s.extra_ports + 1,
            )
        })
        .collect();

    if services.is_empty() {
        return Ok(ComposeOutcome {
            summary: format!("{}: no services with port declarations.", file.display()),
            warnings,
        });
    }

    let project = project::current_project()?;
    let targets: Vec<(String, Protocol)> = services
        .iter()
        .map(|s| (s.name.clone(), s.protocol))
        .collect();
    let names: Vec<String> = targets.iter().map(|(n, _)| n.clone()).collect();

    let (ports, new_flags) = registry::with_lock(registry_path, |r| {
        let mut ports = Vec::with_capacity(targets.len());
        let mut new_flags = Vec::with_capacity(targets.len());
        for (service, protocol) in &targets {
            let existed = r.get(&project, service).is_some();
            let port = allocator::alloc_within_lock(r, &project, service, *protocol)?;
            ports.push(port);
            new_flags.push(!existed);
        }
        Ok((ports, new_flags))
    })?;

    let new_count = new_flags.iter().filter(|n| **n).count();
    let existing_count = targets.len() - new_count;
    let summary = format_compose_summary(
        &file,
        &names,
        &ports,
        &targets.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
        new_count,
        existing_count,
    );
    warnings.sort();
    Ok(ComposeOutcome { summary, warnings })
}

fn discover_compose(cwd: &Path, explicit: Option<&Path>) -> Result<PathBuf, SpoutError> {
    if let Some(p) = explicit {
        return if p.exists() {
            Ok(p.to_owned())
        } else {
            Err(SpoutError::ComposeNotFound)
        };
    }
    for name in COMPOSE_FILENAMES {
        let candidate = cwd.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(SpoutError::ComposeNotFound)
}

fn format_compose_summary(
    file: &Path,
    names: &[String],
    ports: &[u16],
    protocols: &[Protocol],
    new_count: usize,
    existing_count: usize,
) -> String {
    let header = if existing_count == 0 {
        format!(
            "{} → {} service{} allocated.",
            file.display(),
            names.len(),
            if names.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} → {} services ({} new, {} existing).",
            file.display(),
            names.len(),
            new_count,
            existing_count,
        )
    };
    let width = names.iter().map(|n| n.len()).max().unwrap_or(0);
    let rows: Vec<String> = names
        .iter()
        .zip(ports.iter().zip(protocols.iter()))
        .map(|(name, (port, proto))| format!("  {name:<width$}  {port}  {proto}"))
        .collect();
    format!("{header}\n\n{}", rows.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_compose(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    fn basic_compose() -> &'static str {
        r#"
services:
  postgres:
    ports: ["5432"]
  redis:
    ports: ["6379"]
  dns:
    ports: ["53:53/udp"]
"#
    }

    #[test]
    fn discover_finds_docker_compose_yml() {
        let dir = TempDir::new().unwrap();
        write_compose(dir.path(), "docker-compose.yml", basic_compose());
        let got = discover_compose(dir.path(), None).unwrap();
        assert!(got.ends_with("docker-compose.yml"));
    }

    #[test]
    fn discover_falls_through_to_compose_yaml() {
        let dir = TempDir::new().unwrap();
        write_compose(dir.path(), "compose.yaml", basic_compose());
        let got = discover_compose(dir.path(), None).unwrap();
        assert!(got.ends_with("compose.yaml"));
    }

    #[test]
    fn discover_prefers_docker_compose_yml_over_compose_yaml() {
        let dir = TempDir::new().unwrap();
        write_compose(dir.path(), "docker-compose.yml", basic_compose());
        write_compose(dir.path(), "compose.yaml", basic_compose());
        let got = discover_compose(dir.path(), None).unwrap();
        assert!(got.ends_with("docker-compose.yml"));
    }

    #[test]
    fn discover_honours_explicit_path() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("prod.yml");
        std::fs::write(&p, basic_compose()).unwrap();
        let got = discover_compose(dir.path(), Some(&p)).unwrap();
        assert_eq!(got, p);
    }

    #[test]
    fn discover_missing_file_is_compose_not_found() {
        let dir = TempDir::new().unwrap();
        let err = discover_compose(dir.path(), None).unwrap_err();
        assert!(matches!(err, SpoutError::ComposeNotFound));
        assert_eq!(err.exit_code(), 8);
    }

    #[test]
    fn discover_missing_explicit_path_is_compose_not_found() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist.yml");
        let err = discover_compose(dir.path(), Some(&missing)).unwrap_err();
        assert!(matches!(err, SpoutError::ComposeNotFound));
    }

    #[test]
    fn format_summary_one_service_uses_singular() {
        let out = format_compose_summary(
            Path::new("docker-compose.yml"),
            &["api".into()],
            &[20_000],
            &[Protocol::Tcp],
            1,
            0,
        );
        assert!(out.contains("1 service allocated"));
        assert!(out.contains("api"));
        assert!(out.contains("20000"));
        assert!(out.contains("tcp"));
    }

    #[test]
    fn format_summary_mixed_new_and_existing() {
        let out = format_compose_summary(
            Path::new("docker-compose.yml"),
            &["a".into(), "b".into()],
            &[20_000, 20_001],
            &[Protocol::Tcp, Protocol::Udp],
            1,
            1,
        );
        assert!(out.contains("2 services (1 new, 1 existing)"));
        assert!(out.contains("udp"));
    }
}
