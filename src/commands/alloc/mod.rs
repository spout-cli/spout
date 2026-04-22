//! `spout alloc` — single-service allocation (`alloc`) and
//! compose-file-driven batch allocation (`compose`).

use std::path::{Path, PathBuf};

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::project_markers::COMPOSE_FILENAMES;
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

struct Allocation<'a> {
    name: &'a str,
    port: u16,
    protocol: Protocol,
    is_new: bool,
}

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
    let warnings = build_warnings(&services);

    if services.is_empty() {
        return Ok(ComposeOutcome {
            summary: format!("{}: no services with port declarations.", file.display()),
            warnings,
        });
    }

    let allocations = build_allocations(registry_path, &services)?;
    let summary = format_compose_summary(&file, &allocations);
    Ok(ComposeOutcome { summary, warnings })
}

fn build_warnings(services: &[ComposeService]) -> Vec<String> {
    // BTreeMap iteration gives services in alphabetical order already, so
    // the resulting warnings are sorted without any further sort() call.
    services
        .iter()
        .filter(|s| s.extra_ports > 0)
        .map(|s| {
            format!(
                "'{}' declares {} ports; allocating only the first",
                s.name,
                s.extra_ports + 1,
            )
        })
        .collect()
}

fn build_allocations<'a>(
    registry_path: &Path,
    services: &'a [ComposeService],
) -> Result<Vec<Allocation<'a>>, SpoutError> {
    let project = project::current_project()?;
    registry::with_lock(registry_path, |r| {
        services
            .iter()
            .map(|s| {
                let (port, is_new) =
                    allocator::alloc_within_lock(r, &project, &s.name, s.protocol)?;
                Ok(Allocation {
                    name: &s.name,
                    port,
                    protocol: s.protocol,
                    is_new,
                })
            })
            .collect()
    })
}

fn discover_compose(cwd: &Path, explicit: Option<&Path>) -> Result<PathBuf, SpoutError> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_owned())
        } else {
            Err(SpoutError::ComposeNotFound)
        };
    }
    for name in COMPOSE_FILENAMES {
        let candidate = cwd.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(SpoutError::ComposeNotFound)
}

fn format_compose_summary(file: &Path, allocations: &[Allocation<'_>]) -> String {
    let total = allocations.len();
    let new_count = allocations.iter().filter(|a| a.is_new).count();
    let header = if new_count == total {
        format!(
            "{} → {total} service{} allocated.",
            file.display(),
            if total == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} → {total} services ({new_count} new, {} existing).",
            file.display(),
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
            &[Allocation {
                name: "api",
                port: 20_000,
                protocol: Protocol::Tcp,
                is_new: true,
            }],
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
            &[
                Allocation {
                    name: "a",
                    port: 20_000,
                    protocol: Protocol::Tcp,
                    is_new: true,
                },
                Allocation {
                    name: "b",
                    port: 20_001,
                    protocol: Protocol::Udp,
                    is_new: false,
                },
            ],
        );
        assert!(out.contains("2 services (1 new, 1 existing)"));
        assert!(out.contains("udp"));
    }
}
