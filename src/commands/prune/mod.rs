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

use scanner::{format_report, scan, Candidate, StaleReason};

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
        return bulk(path, older_than, &reg, &candidates);
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

fn bulk(
    path: &Path,
    older_than: u64,
    reg: &Registry,
    candidates: &[Candidate<'_>],
) -> Result<String, SpoutError> {
    if candidates.is_empty() {
        return Ok(format_report(reg, candidates));
    }
    let mut out = String::new();
    for c in candidates {
        apply_remove(path, c, older_than)?;
        out.push_str(&format!("  removed {}/{}.\n", c.project, c.service));
    }
    out.push_str(&format!("\nDone: {} removed, 0 kept.", candidates.len()));
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date;
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

    fn iso_days_ago(n: i64) -> String {
        let today_days = date::days_between("1970-01-01", &date::today_iso()).unwrap();
        let (y, m, d) = date::civil_from_days(today_days - n);
        format!("{y:04}-{m:02}-{d:02}")
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
        seed(&mut reg, "p", "fresh", 20_000, &date::today_iso());
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
            &date::today_iso(),
        );
        write_registry(&path, &reg);
        let _ = run(&path, 90, false, true).unwrap();
        let back = registry::read(&path).unwrap();
        assert_eq!(back.history[0].reason, "pruned: project path missing");
    }
}
