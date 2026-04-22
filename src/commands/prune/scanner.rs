//! Scan the registry for stale-entry candidates, and format the
//! dry-run report.

use std::path::Path;

use crate::allocator;
use crate::date;
use crate::format;
use crate::registry::{Entry, Registry};

#[derive(Debug, PartialEq)]
pub(super) enum StaleReason {
    OlderThan(u64),
    PathMissing,
}

pub(super) struct Candidate<'a> {
    pub project: &'a str,
    pub service: &'a str,
    pub entry: &'a Entry,
    pub reason: StaleReason,
}

pub(super) fn scan<'a>(reg: &'a Registry, older_than: u64) -> Vec<Candidate<'a>> {
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

pub(super) fn nothing_to_prune(older_than: u64) -> String {
    format!("Nothing to prune (all registrations < {older_than}d, all project paths present).")
}

pub(super) fn format_report(
    reg: &Registry,
    older_than: u64,
    candidates: &[Candidate<'_>],
) -> String {
    if candidates.is_empty() {
        return nothing_to_prune(older_than);
    }
    let bound = allocator::probe_bound_ports(reg);
    let mut out = String::from("Stale candidates:\n");
    let mut current_project: Option<&str> = None;
    let mut project_count = 0usize;
    for c in candidates {
        if current_project != Some(c.project) {
            let tag = if c.reason == StaleReason::PathMissing {
                "   [path missing]"
            } else {
                ""
            };
            out.push_str(&format!("\n  {}{tag}\n", c.project));
            current_project = Some(c.project);
            project_count += 1;
        }
        let glyph = format::port_status_glyph(bound.contains(&c.entry.port));
        let age = match &c.reason {
            StaleReason::OlderThan(d) => format!("({d}d)"),
            StaleReason::PathMissing => String::new(),
        };
        out.push_str(&format!(
            "    {glyph} {:<16} {}  allocated {}  {age}\n",
            c.service, c.entry.port, c.entry.allocated,
        ));
    }
    let (cn, pn) = (candidates.len(), project_count);
    let (cs, ps) = (
        if cn == 1 { "" } else { "s" },
        if pn == 1 { "" } else { "s" },
    );
    out.push_str(&format!("\n{cn} candidate{cs} across {pn} project{ps}.\n"));
    out.push_str(
        "Rerun `spout prune` to remove interactively, or `spout prune --yes` to skip prompts.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::iso_days_ago;
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

    #[test]
    fn scan_empty_registry_yields_no_candidates() {
        let reg = Registry::default();
        assert!(scan(&reg, 90).is_empty());
    }

    #[test]
    fn scan_age_filters_by_cutoff() {
        let mut reg = Registry::default();
        seed(&mut reg, "p", "fresh", 20_000, &iso_days_ago(5));
        seed(&mut reg, "p", "stale", 20_001, &iso_days_ago(100));
        seed(&mut reg, "p", "older", 20_002, &iso_days_ago(200));
        let got = scan(&reg, 90);
        let names: Vec<_> = got.iter().map(|c| c.service).collect();
        assert_eq!(names, vec!["older", "stale"]);
    }

    #[test]
    fn scan_respects_older_than_cutoff() {
        let mut reg = Registry::default();
        seed(&mut reg, "p", "mid", 20_001, &iso_days_ago(100));
        seed(&mut reg, "p", "old", 20_002, &iso_days_ago(200));
        let got = scan(&reg, 150);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].service, "old");
    }

    #[test]
    fn scan_flags_path_missing_identities() {
        let (tmp, _) = temp_path();
        let gone = tmp.path().join("gone").to_str().unwrap().to_owned();
        let mut reg = Registry::default();
        seed(&mut reg, &gone, "svc", 20_000, &date::today_iso());
        let got = scan(&reg, 90);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].reason, StaleReason::PathMissing);
    }

    #[test]
    fn nothing_to_prune_uses_the_actual_cutoff() {
        assert!(nothing_to_prune(30).contains("< 30d"));
        assert!(nothing_to_prune(180).contains("< 180d"));
    }

    #[test]
    fn format_report_groups_by_project_and_reports_count() {
        let mut reg = Registry::default();
        seed(&mut reg, "proj-a", "alpha", 20_001, &iso_days_ago(100));
        seed(&mut reg, "proj-a", "beta", 20_002, &iso_days_ago(150));
        seed(&mut reg, "proj-b", "gamma", 20_003, &iso_days_ago(200));
        let candidates = scan(&reg, 90);
        let out = format_report(&reg, 90, &candidates);
        assert!(out.contains("proj-a"));
        assert!(out.contains("proj-b"));
        assert!(out.contains("3 candidates across 2 projects"), "got {out}");
    }
}
