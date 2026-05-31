//! Plain `ls` layout — the compact one-liner.
//!
//! FROZEN: this is the byte-for-byte output scripts and agents depend on when
//! stdout is not an interactive terminal. Do not change its shape. Width is
//! computed on `len()` (a pre-existing choice); the rich layout, not this one,
//! is where display-width correctness lives.

use std::collections::{HashMap, HashSet};

use super::{port_status_glyph, sorted_services};
use crate::registry::{Entry, Registry};

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
            for (svc, entry) in sorted_services(s) {
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

    fn one(svc: &str, port: u16) -> HashMap<String, Entry> {
        let mut m = HashMap::new();
        m.insert(svc.to_owned(), entry(port, "2026-04-21"));
        m
    }

    #[test]
    fn project_block_prefixes_bound_port_with_solid_circle() {
        let bound: HashSet<u16> = [20_000].into_iter().collect();
        let out = project_block("proj", Some(&one("postgres", 20_000)), &bound);
        assert!(out.contains("● postgres"));
        assert!(!out.contains("○ postgres"));
    }

    #[test]
    fn project_block_prefixes_free_port_with_open_circle() {
        let out = project_block("proj", Some(&one("postgres", 20_000)), &HashSet::new());
        assert!(out.contains("○ postgres"));
        assert!(!out.contains("● postgres"));
    }

    #[test]
    fn all_empty_registry_is_descriptive() {
        assert_eq!(
            all(&Registry::default(), &HashSet::new()),
            "(no registrations)"
        );
    }

    #[test]
    fn plain_layout_emits_no_ansi_escapes() {
        // The contract scripts/agents depend on.
        let out = project_block("proj", Some(&one("postgres", 20_000)), &HashSet::new());
        assert!(!out.contains('\x1b'), "plain leaked an escape: {out:?}");
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
        let tcp_pos = out.find("postgres").unwrap();
        let udp_pos = out.find("dns").unwrap();
        assert!(tcp_pos < udp_pos, "expected tcp before udp, got:\n{out}");
    }
}
