//! `spout prune` — surface and optionally remove stale registrations.
//! Candidates: `allocated` older than the cutoff, or an absolute-path
//! identity whose directory is gone. Offline-only. Three modes:
//! `--dry-run`, interactive (`[y/N/q/!]` per entry), `--yes` bulk.

mod scanner;

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::format;
use crate::registry::{self, Registry};

use scanner::{format_report, nothing_to_prune, scan, Candidate, StaleReason};

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
        return Ok(format_report(&reg, older_than, &candidates));
    }
    if candidates.is_empty() {
        return Ok(nothing_to_prune(older_than));
    }
    if yes {
        return bulk(path, older_than, &candidates);
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

// Candidates must be non-empty — caller guarantees via `run`'s empty-check.
fn bulk(path: &Path, older_than: u64, candidates: &[Candidate<'_>]) -> Result<String, SpoutError> {
    let targets: Vec<(String, String, String)> = candidates
        .iter()
        .map(|c| {
            (
                c.project.to_owned(),
                c.service.to_owned(),
                reason_for(&c.reason, older_than),
            )
        })
        .collect();
    registry::with_lock(path, |r| {
        for (project, service, reason) in &targets {
            r.remove(project, service, reason);
        }
        Ok(())
    })?;
    let mut out = String::new();
    for (project, service, _) in &targets {
        out.push_str(&format!("  removed {project}/{service}.\n"));
    }
    out.push_str(&done_line(targets.len(), 0));
    Ok(out)
}

// Candidates must be non-empty — caller guarantees via `run`'s empty-check.
fn interactive(
    path: &Path,
    older_than: u64,
    reg: &Registry,
    candidates: &[Candidate<'_>],
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
) -> Result<(), SpoutError> {
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
    writeln!(stdout, "{}", done_line(removed, kept)).map_err(io_err)?;
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
    let reason = reason_for(&c.reason, older_than);
    let (project, service) = (c.project.to_owned(), c.service.to_owned());
    registry::with_lock(path, |r| {
        r.remove(&project, &service, &reason);
        Ok(())
    })
}

fn reason_for(r: &StaleReason, older_than: u64) -> String {
    match r {
        StaleReason::OlderThan(_) => format!("pruned: stale (older than {older_than}d)"),
        StaleReason::PathMissing => "pruned: project path missing".to_owned(),
    }
}

fn done_line(removed: usize, kept: usize) -> String {
    format!("\nDone: {removed} removed, {kept} kept.")
}

fn io_err(e: io::Error) -> SpoutError {
    SpoutError::Io(format!("prune: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::{iso_days_ago, today_iso};
    use crate::protocol::Protocol;
    use crate::registry::Entry;
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

    fn seed_stale(path: &std::path::Path) -> Registry {
        let mut reg = Registry::default();
        seed(&mut reg, "p", "old-a", 20_001, &iso_days_ago(120));
        seed(&mut reg, "p", "old-b", 20_002, &iso_days_ago(150));
        seed(&mut reg, "p", "old-c", 20_003, &iso_days_ago(200));
        write_registry(path, &reg);
        registry::read(path).unwrap()
    }

    #[test]
    fn dry_run_reports_candidates() {
        let (_dir, path) = temp_path();
        let _ = seed_stale(&path);
        let out = run(&path, 90, true, false).unwrap();
        assert!(out.contains("3 candidates across 1 project"), "got {out}");
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
        let mut stdin: &[u8] = b"y\nq\n";
        let mut stdout: Vec<u8> = Vec::new();
        interactive(&path, 90, &reg, &candidates, &mut stdin, &mut stdout).unwrap();
        let back = registry::read(&path).unwrap();
        assert_eq!(back.projects.get("p").unwrap().len(), 2);
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
        assert!(!back.projects.contains_key("p"));
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("3 removed, 0 kept"), "got {text}");
    }

    #[test]
    fn bulk_removes_all_and_records_reasons() {
        let (_dir, path) = temp_path();
        let _ = seed_stale(&path);
        let out = run(&path, 90, false, true).unwrap();
        assert!(out.contains("3 removed, 0 kept"), "got {out}");
        let back = registry::read(&path).unwrap();
        assert!(back.projects.is_empty());
        assert!(back
            .history
            .iter()
            .all(|h| h.reason == "pruned: stale (older than 90d)"));
    }

    #[test]
    fn bulk_with_no_candidates_says_nothing_to_prune() {
        let (_dir, path) = temp_path();
        let mut reg = Registry::default();
        seed(&mut reg, "p", "fresh", 20_000, &today_iso());
        write_registry(&path, &reg);
        let out = run(&path, 90, false, true).unwrap();
        assert!(out.contains("Nothing to prune"), "got {out}");
    }

    #[test]
    fn bulk_path_missing_uses_path_reason_string() {
        let (tmp, path) = temp_path();
        let gone = tmp.path().join("gone");
        let mut reg = Registry::default();
        seed(
            &mut reg,
            gone.to_str().unwrap(),
            "svc",
            20_000,
            &today_iso(),
        );
        write_registry(&path, &reg);
        let _ = run(&path, 90, false, true).unwrap();
        let back = registry::read(&path).unwrap();
        assert_eq!(back.history[0].reason, "pruned: project path missing");
    }
}
