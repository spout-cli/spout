//! `spout prune` — surface and optionally remove stale registrations.
//!
//! Candidates are entries where `allocated` is older than the cutoff, OR
//! whose project identity is an absolute filesystem path that no longer
//! exists. Offline-only: no network, no git-remote resolution in this
//! stage.
//!
//! The modes are split one per public function so each stays small:
//! `dry_run` (this commit), interactive stdin (next), and `--yes` bulk
//! (after that).

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

/// Entry point dispatched from `main.rs`. Only `dry_run=true` produces
/// real output today; the other modes will land in the next two commits.
pub fn run(path: &Path, older_than: u64, dry_run: bool, _yes: bool) -> Result<String, SpoutError> {
    let reg = registry::read(path)?;
    let candidates = scan(&reg, older_than);

    if dry_run {
        return Ok(format_report(&reg, &candidates));
    }
    // Interactive and --yes modes arrive in Commit 4 and Commit 5.
    Err(SpoutError::RegistryCorrupt(
        "interactive prune not yet implemented — use --dry-run".to_owned(),
    ))
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
        days_since_epoch_to_iso(today_days - n)
    }

    fn days_since_epoch_to_iso(days: i64) -> String {
        // Inline Hinnant's civil_from_days; the one in date.rs is private.
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
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

    #[test]
    fn non_dry_run_modes_return_not_yet_implemented() {
        let (_dir, path) = temp_path();
        let reg = Registry::default();
        write_registry(&path, &reg);
        assert!(run(&path, 90, false, false).is_err());
        assert!(run(&path, 90, false, true).is_err());
    }
}
