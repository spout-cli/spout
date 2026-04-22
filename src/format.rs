//! Plain-text rendering for `spout ls` and `spout whois`.
//!
//! Kept separate from `commands.rs` (which owns business logic) and
//! `tui.rs` (which owns Ratatui) so each stays comfortably under the
//! 400-line cap. TUI and plain-text share `port_status_glyph` — the
//! one-source-of-truth for the `●`/`○` convention.

use std::collections::{HashMap, HashSet};

use crate::registry::{Entry, HistoryEntry, Registry};

/// `●` = bound on OS, `○` = free. Shared by the plain-text ls and the
/// TUI so the two renderers agree on the convention.
pub fn port_status_glyph(bound: bool) -> &'static str {
    if bound {
        "●"
    } else {
        "○"
    }
}

pub fn all(reg: &Registry, bound: &HashSet<u16>) -> String {
    if reg.projects.is_empty() {
        return String::from("(no registrations)");
    }
    let mut sorted: Vec<_> = reg.projects.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    sorted
        .into_iter()
        .map(|(project, services)| project_block(project, Some(services), bound))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn project_block(
    project: &str,
    services: Option<&HashMap<String, Entry>>,
    bound: &HashSet<u16>,
) -> String {
    let mut out = String::from(project);
    out.push('\n');
    match services {
        Some(s) if !s.is_empty() => {
            let width = s.keys().map(|k| k.len()).max().unwrap_or(0);
            let mut sorted: Vec<_> = s.iter().collect();
            sorted.sort_by(|a, b| (a.1.protocol, a.0).cmp(&(b.1.protocol, b.0)));
            for (svc, entry) in sorted {
                let status = port_status_glyph(bound.contains(&entry.port));
                out.push_str(&format!(
                    "  {} {:<width$}  {}/{}  (since {})\n",
                    status,
                    svc,
                    entry.port,
                    entry.protocol,
                    entry.allocated,
                    width = width
                ));
            }
        }
        _ => out.push_str("  (no registrations)\n"),
    }
    out.trim_end().to_owned()
}

pub fn history(entries: &[&HistoryEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            format!(
                "{}/{}: was {}/{}  (allocated {}, released {} — {})",
                e.port, e.protocol, e.project, e.service, e.allocated, e.released, e.reason
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(port: u16, allocated: &str) -> Entry {
        Entry {
            port,
            allocated: allocated.to_owned(),
            protocol: crate::protocol::Protocol::default(),
        }
    }

    #[test]
    fn project_block_prefixes_bound_port_with_solid_circle() {
        let mut services = HashMap::new();
        services.insert("postgres".to_owned(), entry(20_000, "2026-04-21"));
        let bound: HashSet<u16> = [20_000].into_iter().collect();
        let out = project_block("proj", Some(&services), &bound);
        assert!(out.contains("● postgres"));
        assert!(!out.contains("○ postgres"));
    }

    #[test]
    fn project_block_prefixes_free_port_with_open_circle() {
        let mut services = HashMap::new();
        services.insert("postgres".to_owned(), entry(20_000, "2026-04-21"));
        let bound: HashSet<u16> = HashSet::new();
        let out = project_block("proj", Some(&services), &bound);
        assert!(out.contains("○ postgres"));
        assert!(!out.contains("● postgres"));
    }

    #[test]
    fn all_empty_registry_is_descriptive() {
        let reg = Registry::default();
        let bound = HashSet::new();
        assert_eq!(all(&reg, &bound), "(no registrations)");
    }

    #[test]
    fn project_block_shows_protocol_suffix_on_every_row() {
        use crate::protocol::Protocol;
        let mut services = HashMap::new();
        services.insert(
            "postgres".to_owned(),
            Entry {
                port: 20_000,
                allocated: "2026-04-21".into(),
                protocol: Protocol::Tcp,
            },
        );
        services.insert(
            "dns".to_owned(),
            Entry {
                port: 20_053,
                allocated: "2026-04-21".into(),
                protocol: Protocol::Udp,
            },
        );
        let out = project_block("proj", Some(&services), &HashSet::new());
        assert!(out.contains("20000/tcp"), "got {out}");
        assert!(out.contains("20053/udp"), "got {out}");
        // TCP rows sort before UDP at the same port region.
        let tcp_pos = out.find("postgres").unwrap();
        let udp_pos = out.find("dns").unwrap();
        assert!(tcp_pos < udp_pos, "expected tcp before udp, got:\n{out}");
    }
}
