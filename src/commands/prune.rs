//! `spout prune` — surface and optionally remove stale registrations.
//! Candidates: `allocated` older than the cutoff, or an absolute-path
//! identity whose directory is gone. Offline-only. Three modes:
//! `--dry-run`, interactive (`[y/N/q/!]` per entry), `--yes` bulk.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::allocator;
use crate::date;
use crate::error::SpoutError;
use crate::format;
use crate::registry::{self, Entry, Registry};

const DEFAULT_OLDER_THAN: u64 = 90;

#[derive(Debug, PartialEq)]
enum StaleReason {
    OlderThan(u64),
    PathMissing,
}

struct Candidate<'a> {
    project: &'a str,
    service: &'a str,
    entry: &'a Entry,
    reason: StaleReason,
}

#[derive(Debug, PartialEq)]
enum Response {
    Yes,
    No,
    Quit,
    YesToAll,
}

/// Entry point dispatched from `main.rs`.
pub fn run(path: &Path, older_than: u64, dry_run: bool, yes: bool) -> Result<String, SpoutError> {
    let reg = registry::read(path)?;
    let candidates = scan(&reg, older_than);

    if dry_run {
        return Ok(format_report(&reg, &candidates));
    }
    if yes {
        return Err(SpoutError::RegistryCorrupt(
            "--yes bulk prune not yet implemented".to_owned(),
        ));
    }
    let (stdin, stdout) = (io::stdin(), io::stdout());
    interactive(
        path,
        older_than,
        &reg,
        &candidates,
        &mut stdin.lock(),
        &mut stdout.lock(),
    )?;
    Ok(String::new())
}

fn interactive(
    path: &Path,
    older_than: u64,
    reg: &Registry,
    candidates: &[Candidate<'_>],
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
) -> Result<(), SpoutError> {
    if candidates.is_empty() {
        writeln!(stdout, "{}", format_report(reg, candidates)).map_err(io_err)?;
        return Ok(());
    }
    let bound = allocator::probe_bound_ports(reg);
    let mut yes_to_all = false;
    let (mut removed, mut kept) = (0usize, 0usize);
    for c in candidates {
        let response = if yes_to_all {
            Response::Yes
        } else {
            prompt(c, &bound, stdin, stdout)?
        };
        match response {
            Response::Yes | Response::YesToAll => {
                if response == Response::YesToAll {
                    yes_to_all = true;
                }
                apply_remove(path, c, older_than)?;
                writeln!(stdout, "  removed.").map_err(io_err)?;
                removed += 1;
            }
            Response::No => kept += 1,
            Response::Quit => break,
        }
    }
    writeln!(stdout, "\nDone: {removed} removed, {kept} kept.").map_err(io_err)?;
    Ok(())
}

fn prompt(
    c: &Candidate<'_>,
    bound: &HashSet<u16>,
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
) -> Result<Response, SpoutError> {
    let glyph = format::port_status_glyph(bound.contains(&c.entry.port));
    let suffix = if c.reason == StaleReason::PathMissing {
        " [path missing]"
    } else {
        ""
    };
    let age = match &c.reason {
        StaleReason::OlderThan(d) => format!("{d}d ago"),
        StaleReason::PathMissing => "path missing".to_owned(),
    };
    writeln!(stdout, "Remove {}/{}?{suffix}", c.project, c.service).map_err(io_err)?;
    writeln!(stdout, "  allocated {} ({age}, {glyph})", c.entry.allocated).map_err(io_err)?;
    write!(stdout, "  [y/N/q/!] ").map_err(io_err)?;
    stdout.flush().map_err(io_err)?;
    let mut line = String::new();
    if stdin.read_line(&mut line).map_err(io_err)? == 0 {
        return Ok(Response::Quit);
    }
    Ok(match line.trim() {
        "y" | "Y" => Response::Yes,
        "q" | "Q" => Response::Quit,
        "!" => Response::YesToAll,
        _ => Response::No,
    })
}

fn apply_remove(path: &Path, c: &Candidate<'_>, older_than: u64) -> Result<(), SpoutError> {
    let reason = match c.reason {
        StaleReason::OlderThan(_) => format!("pruned: stale (older than {older_than}d)"),
        StaleReason::PathMissing => "pruned: project path missing".to_owned(),
    };
    let (project, service) = (c.project.to_owned(), c.service.to_owned());
    registry::with_lock(path, |r| {
        r.remove(&project, &service, &reason);
        Ok(())
    })
}

fn io_err(e: io::Error) -> SpoutError {
    SpoutError::RegistryCorrupt(format!("prune I/O: {e}"))
}

fn scan<'a>(reg: &'a Registry, older_than: u64) -> Vec<Candidate<'a>> {
    let mut out = Vec::new();
    for (project, services) in &reg.projects {
        let path_missing = project.starts_with('/') && !Path::new(project).exists();
        for (service, entry) in services {
            let reason = if path_missing {
                Some(StaleReason::PathMissing)
            } else {
                date::days_ago(&entry.allocated)
                    .filter(|&d| d > older_than as i64)
                    .map(|d| StaleReason::OlderThan(d as u64))
            };
            if let Some(reason) = reason {
                out.push(Candidate {
                    project,
                    service,
                    entry,
                    reason,
                });
            }
        }
    }
    out.sort_by(|a, b| (a.project, a.service).cmp(&(b.project, b.service)));
    out
}

fn format_report(reg: &Registry, candidates: &[Candidate<'_>]) -> String {
    if candidates.is_empty() {
        return format!(
            "Nothing to prune (all registrations < {DEFAULT_OLDER_THAN}d, all project paths present)."
        );
    }
    let bound = allocator::probe_bound_ports(reg);
    let mut out = String::from("Stale candidates:\n");
    let mut current_project: Option<&str> = None;
    let mut project_count = 0usize;
    for c in candidates {
        if current_project != Some(c.project) {
            out.push('\n');
            let tag = if c.reason == StaleReason::PathMissing {
                "   [path missing]"
            } else {
                ""
            };
            out.push_str(&format!("  {}{}\n", c.project, tag));
            current_project = Some(c.project);
            project_count += 1;
        }
        let glyph = format::port_status_glyph(bound.contains(&c.entry.port));
        let age = match &c.reason {
            StaleReason::OlderThan(d) => format!("({d}d)"),
            StaleReason::PathMissing => String::new(),
        };
        out.push_str(&format!(
            "    {} {:<16} {}  allocated {}  {}\n",
            glyph, c.service, c.entry.port, c.entry.allocated, age
        ));
    }
    out.push_str(&format!(
        "\n{} candidate{} across {} project{}.\n",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" },
        project_count,
        if project_count == 1 { "" } else { "s" },
    ));
    out.push_str(
        "Rerun `spout prune` to remove interactively, or `spout prune --yes` to skip prompts.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;
    use tempfile::TempDir;

    fn temp_path() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    fn seed(reg: &mut Registry, project: &str, service: &str, port: u16, allocated: &str) {
        reg.projects.entry(project.to_owned()).or_default().insert(
            service.to_owned(),
            Entry {
                port,
                allocated: allocated.to_owned(),
                protocol: Protocol::default(),
            },
        );
    }

    fn write_registry(path: &std::path::Path, reg: &Registry) {
        let json = serde_json::to_string(reg).unwrap();
        std::fs::write(path, json).unwrap();
    }

    fn iso_days_ago(n: i64) -> String {
        let today_days = date::days_between("1970-01-01", &date::today_iso()).unwrap();
        let (y, m, d) = date::civil_from_days(today_days - n);
        format!("{y:04}-{m:02}-{d:02}")
    }

    #[test]
    fn dry_run_with_no_stale_entries_says_nothing_to_prune() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "s", 20_000, &date::today_iso());
        write_registry(&path, &reg);
        let out = run(&path, 90, true, false).unwrap();
        assert!(out.contains("Nothing to prune"), "got {out}");
    }

    #[test]
    fn dry_run_surfaces_age_triggered_candidates() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "fresh", 20_000, &iso_days_ago(5));
        seed(&mut reg, "p", "stale", 20_001, &iso_days_ago(100));
        seed(&mut reg, "p", "older", 20_002, &iso_days_ago(200));
        write_registry(&path, &reg);
        let out = run(&path, 90, true, false).unwrap();
        assert!(!out.contains("fresh"), "got {out}");
        assert!(out.contains("stale"), "got {out}");
        assert!(out.contains("older"), "got {out}");
    }

    #[test]
    fn dry_run_respects_older_than_cutoff() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "mid", 20_001, &iso_days_ago(100));
        seed(&mut reg, "p", "old", 20_002, &iso_days_ago(200));
        write_registry(&path, &reg);
        let out = run(&path, 150, true, false).unwrap();
        assert!(!out.contains("mid"), "got {out}");
        assert!(out.contains("old"), "got {out}");
    }

    #[test]
    fn dry_run_surfaces_path_missing_candidate() {
        let (tmp, path) = temp_path();
        let gone = tmp.path().join("gone-for-good");
        let mut reg = Registry::default();
        seed(
            &mut reg,
            gone.to_str().unwrap(),
            "svc",
            20_000,
            &date::today_iso(),
        );
        write_registry(&path, &reg);
        let out = run(&path, 90, true, false).unwrap();
        assert!(out.contains("path missing"), "got {out}");
        assert!(out.contains("svc"), "got {out}");
    }

    #[test]
    fn dry_run_groups_by_project_and_reports_count() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "proj-a", "alpha", 20_001, &iso_days_ago(100));
        seed(&mut reg, "proj-a", "beta", 20_002, &iso_days_ago(150));
        seed(&mut reg, "proj-b", "gamma", 20_003, &iso_days_ago(200));
        write_registry(&path, &reg);
        let out = run(&path, 90, true, false).unwrap();
        assert!(out.contains("proj-a"));
        assert!(out.contains("proj-b"));
        assert!(out.contains("3 candidates across 2 projects"), "got {out}");
    }

    fn seed_stale(path: &std::path::Path) -> Registry {
        let mut reg = Registry::default();
        seed(&mut reg, "p", "old-a", 20_001, &iso_days_ago(120));
        seed(&mut reg, "p", "old-b", 20_002, &iso_days_ago(150));
        seed(&mut reg, "p", "old-c", 20_003, &iso_days_ago(200));
        write_registry(path, &reg);
        registry::read(path).unwrap()
    }

    #[test]
    fn interactive_y_removes_and_writes_rich_reason() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "old", 20_001, &iso_days_ago(200));
        write_registry(&path, &reg);
        let reg = registry::read(&path).unwrap();
        let candidates = scan(&reg, 90);
        let mut stdin: &[u8] = b"y\n";
        let mut stdout: Vec<u8> = Vec::new();
        interactive(&path, 90, &reg, &candidates, &mut stdin, &mut stdout).unwrap();
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("removed."), "got {text}");
        let back = registry::read(&path).unwrap();
        assert!(back.get("p", "old").is_none());
        assert_eq!(back.history[0].reason, "pruned: stale (older than 90d)");
    }

    #[test]
    fn interactive_bare_enter_keeps_the_entry() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "old", 20_001, &iso_days_ago(200));
        write_registry(&path, &reg);
        let reg = registry::read(&path).unwrap();
        let candidates = scan(&reg, 90);
        let mut stdin: &[u8] = b"\n";
        let mut stdout: Vec<u8> = Vec::new();
        interactive(&path, 90, &reg, &candidates, &mut stdin, &mut stdout).unwrap();
        let back = registry::read(&path).unwrap();
        assert_eq!(back.get("p", "old"), Some(20_001));
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("0 removed, 1 kept"), "got {text}");
    }

    #[test]
    fn interactive_q_stops_without_touching_remaining() {
        let (_dir, path) = temp_path();
        let reg = seed_stale(&path);
        let candidates = scan(&reg, 90);
        assert_eq!(candidates.len(), 3);
        let mut stdin: &[u8] = b"y\nq\n";
        let mut stdout: Vec<u8> = Vec::new();
        interactive(&path, 90, &reg, &candidates, &mut stdin, &mut stdout).unwrap();
        let back = registry::read(&path).unwrap();
        let svcs = back.projects.get("p").unwrap();
        assert_eq!(svcs.len(), 2, "only the first should have been removed");
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("1 removed"), "got {text}");
    }

    #[test]
    fn interactive_bang_applies_yes_to_all_remaining() {
        let (_dir, path) = temp_path();
        let reg = seed_stale(&path);
        let candidates = scan(&reg, 90);
        let mut stdin: &[u8] = b"!\n";
        let mut stdout: Vec<u8> = Vec::new();
        interactive(&path, 90, &reg, &candidates, &mut stdin, &mut stdout).unwrap();
        let back = registry::read(&path).unwrap();
        assert!(!back.projects.contains_key("p"), "all entries removed");
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("3 removed, 0 kept"), "got {text}");
    }
}
