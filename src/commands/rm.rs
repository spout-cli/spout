//! `spout rm` — single-service removal (default) and whole-project
//! removal (`--project`, no service). The whole-project path takes a
//! single `[y/N]` confirmation by default; `--yes` skips it,
//! `--dry-run` previews without changes.

use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::error::SpoutError;
use crate::project;
use crate::registry::{self, Registry};

#[derive(Default, Clone, Copy)]
pub struct RmOptions {
    pub yes: bool,
    pub dry_run: bool,
}

pub enum RmTarget {
    /// Single service. `project` of `None` means "current project".
    Service {
        name: String,
        project: Option<String>,
    },
    /// Every service in the named project.
    Project { name: String },
}

pub fn run(registry_path: &Path, target: RmTarget, opts: RmOptions) -> Result<String, SpoutError> {
    match target {
        RmTarget::Service { name, project } => rm_one(registry_path, &name, project.as_deref()),
        RmTarget::Project { name } => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            rm_project(
                registry_path,
                &name,
                opts,
                &mut stdin.lock(),
                &mut stdout.lock(),
            )
        }
    }
}

fn rm_one(
    registry_path: &Path,
    service: &str,
    project: Option<&str>,
) -> Result<String, SpoutError> {
    let project = match project {
        Some(p) => p.to_owned(),
        None => project::current_project()?,
    };
    registry::with_lock(registry_path, |r| {
        r.remove(&project, service, "user requested")
            .ok_or(SpoutError::ServiceNotRegistered)?;
        Ok(())
    })?;
    Ok(String::new())
}

fn rm_project(
    registry_path: &Path,
    project: &str,
    opts: RmOptions,
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
) -> Result<String, SpoutError> {
    let reg = registry::read(registry_path)?;
    let services = list_services(&reg, project)?;
    let preview = format_block(project, &services);

    if opts.dry_run {
        return Ok(preview);
    }
    if !opts.yes && !confirm(stdin, stdout, &preview)? {
        return Ok(format!("Cancelled. No changes to '{project}'."));
    }
    let removed = registry::with_lock(registry_path, |r| {
        Ok(r.remove_project(project, "user requested (project rm)"))
    })?;
    Ok(format!("Removed {removed} service(s) from '{project}'."))
}

fn list_services(reg: &Registry, project: &str) -> Result<Vec<(String, u16)>, SpoutError> {
    let services = reg
        .projects
        .get(project)
        .filter(|s| !s.is_empty())
        .ok_or(SpoutError::ServiceNotRegistered)?;
    let mut sorted: Vec<_> = services.iter().map(|(n, e)| (n.clone(), e.port)).collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(sorted)
}

fn format_block(project: &str, services: &[(String, u16)]) -> String {
    let width = services.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let rows: Vec<String> = services
        .iter()
        .map(|(name, port)| format!("  {name:<width$}  {port}"))
        .collect();
    format!(
        "Remove all {} service(s) for '{project}'?\n{}",
        services.len(),
        rows.join("\n"),
    )
}

fn confirm(
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
    block: &str,
) -> Result<bool, SpoutError> {
    let io_err = |e: io::Error| SpoutError::Io(format!("rm: {e}"));
    writeln!(stdout, "{block}").map_err(io_err)?;
    write!(stdout, "[y/N] ").map_err(io_err)?;
    stdout.flush().map_err(io_err)?;
    let mut line = String::new();
    if stdin.read_line(&mut line).map_err(io_err)? == 0 {
        return Ok(false);
    }
    Ok(matches!(line.trim(), "y" | "Y"))
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

    fn seed(path: &Path, project: &str, services: &[(&str, u16)]) {
        registry::with_lock(path, |r| {
            for (name, port) in services {
                r.set(project, name, *port, Protocol::default());
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn rm_one_in_current_project() {
        let (_dir, path) = temp_registry();
        let proj = project::current_project().unwrap();
        seed(&path, &proj, &[("postgres", 20_000)]);
        rm_one(&path, "postgres", None).unwrap();
        assert!(registry::read(&path)
            .unwrap()
            .get(&proj, "postgres")
            .is_none());
    }

    #[test]
    fn rm_one_in_named_project_other_than_current() {
        let (_dir, path) = temp_registry();
        seed(&path, "named-proj", &[("redis", 20_001)]);
        rm_one(&path, "redis", Some("named-proj")).unwrap();
        assert!(!registry::read(&path)
            .unwrap()
            .projects
            .contains_key("named-proj"));
    }

    #[test]
    fn rm_project_yes_removes_all_in_one_lock() {
        let (_dir, path) = temp_registry();
        seed(
            &path,
            "myapp",
            &[("postgres", 20_000), ("redis", 20_001), ("api", 20_002)],
        );
        let mut stdin: &[u8] = b"";
        let mut stdout: Vec<u8> = Vec::new();
        let opts = RmOptions {
            yes: true,
            dry_run: false,
        };
        let out = rm_project(&path, "myapp", opts, &mut stdin, &mut stdout).unwrap();
        assert!(
            out.contains("Removed 3 service(s) from 'myapp'"),
            "got {out}"
        );
        let back = registry::read(&path).unwrap();
        assert!(!back.projects.contains_key("myapp"));
    }

    #[test]
    fn rm_project_records_distinct_history_reason() {
        let (_dir, path) = temp_registry();
        seed(&path, "myapp", &[("postgres", 20_000)]);
        let mut stdin: &[u8] = b"";
        let mut stdout: Vec<u8> = Vec::new();
        let opts = RmOptions {
            yes: true,
            dry_run: false,
        };
        rm_project(&path, "myapp", opts, &mut stdin, &mut stdout).unwrap();
        let history = registry::read(&path).unwrap().history;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reason, "user requested (project rm)");
    }

    #[test]
    fn rm_project_dry_run_lists_without_removing() {
        let (_dir, path) = temp_registry();
        seed(&path, "myapp", &[("postgres", 20_000)]);
        let mut stdin: &[u8] = b"";
        let mut stdout: Vec<u8> = Vec::new();
        let opts = RmOptions {
            yes: false,
            dry_run: true,
        };
        let out = rm_project(&path, "myapp", opts, &mut stdin, &mut stdout).unwrap();
        assert!(out.contains("Remove all 1 service"));
        assert!(out.contains("postgres"));
        // Registry still has the service.
        assert!(registry::read(&path)
            .unwrap()
            .get("myapp", "postgres")
            .is_some());
    }

    #[test]
    fn rm_project_n_response_keeps_everything() {
        let (_dir, path) = temp_registry();
        seed(&path, "myapp", &[("postgres", 20_000)]);
        let mut stdin: &[u8] = b"\n";
        let mut stdout: Vec<u8> = Vec::new();
        let opts = RmOptions::default();
        let out = rm_project(&path, "myapp", opts, &mut stdin, &mut stdout).unwrap();
        assert!(out.contains("Cancelled"), "got {out}");
        assert!(registry::read(&path)
            .unwrap()
            .get("myapp", "postgres")
            .is_some());
    }

    #[test]
    fn rm_project_y_response_removes_everything() {
        let (_dir, path) = temp_registry();
        seed(&path, "myapp", &[("postgres", 20_000), ("redis", 20_001)]);
        let mut stdin: &[u8] = b"y\n";
        let mut stdout: Vec<u8> = Vec::new();
        let opts = RmOptions::default();
        rm_project(&path, "myapp", opts, &mut stdin, &mut stdout).unwrap();
        assert!(!registry::read(&path)
            .unwrap()
            .projects
            .contains_key("myapp"));
    }

    #[test]
    fn rm_project_unknown_project_errors_one() {
        let (_dir, path) = temp_registry();
        let mut stdin: &[u8] = b"";
        let mut stdout: Vec<u8> = Vec::new();
        let err = rm_project(
            &path,
            "no-such-project",
            RmOptions::default(),
            &mut stdin,
            &mut stdout,
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }
}
