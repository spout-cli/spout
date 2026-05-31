//! Rich `ls` layout — coloured, columned, icon-prefixed.
//!
//! The human terminal view that replaced the old ratatui TUI. Renders inline
//! and returns. Column widths are measured with `unicode-width` so double-width
//! emoji icons (`SPOUT_ICONS`) don't knock the columns out of alignment. All
//! colour goes through [`Palette::paint`], so an inactive palette yields plain,
//! escape-free text identical in shape to the coloured version.

use std::collections::{HashMap, HashSet};

use unicode_width::UnicodeWidthStr;

use super::{port_status_glyph, sorted_services, Palette};
use crate::registry::{Entry, Registry};
use crate::services::{env_var_name, service_icon};

/// Computed column widths for the rich table. PROTO is always `tcp`/`udp`
/// (fixed 5 incl. header) and ALLOCATED is always `YYYY-MM-DD` (fixed 10),
/// so only these three vary with content.
struct RichWidths {
    name: usize,
    port: usize,
    env: usize,
}

/// Display width of a service's name cell, including the icon prefix when
/// icons are active. Uses `unicode-width` because an emoji occupies ~2
/// terminal cells but only 1 `char`, so `len()` would misalign the columns.
fn name_cell_width(svc: &str, with_icons: bool) -> usize {
    let icon_w = if with_icons {
        service_icon(svc).map_or(0, |i| format!("{i} ").width())
    } else {
        0
    };
    icon_w + svc.width()
}

fn rich_widths(entries: &[(&String, &Entry)], with_icons: bool) -> RichWidths {
    let mut w = RichWidths {
        name: "SERVICE".len(),
        port: "PORT".len(),
        env: "ENV VAR".len(),
    };
    for (svc, e) in entries {
        w.name = w.name.max(name_cell_width(svc, with_icons));
        w.port = w.port.max(e.port.to_string().len());
        w.env = w.env.max(env_var_name(svc).len());
    }
    w
}

/// One render's shared state, so the per-row method stays within the
/// four-argument limit.
struct RichView<'a> {
    bound: &'a HashSet<u16>,
    palette: &'a Palette,
    widths: RichWidths,
}

impl RichView<'_> {
    /// Column header followed by a horizontal rule. The two-space indent
    /// reserves the `● ` glyph column each data row carries, so SERVICE lines
    /// up over the names. The rule (not a blank line) ties the labels to the
    /// rows below: without it the column header floats orphaned above the
    /// first project header. Mirrors the old TUI's title underline.
    fn header(&self) -> String {
        let w = &self.widths;
        let line = format!(
            "  {:<nw$}  {:<pw$}  {:<5}  {:<ew$}  {}",
            "SERVICE",
            "PORT",
            "PROTO",
            "ENV VAR",
            "ALLOCATED",
            nw = w.name,
            pw = w.port,
            ew = w.env,
        );
        // Rule spans the FULL width including the 2-char glyph gutter: the
        // `●`/`○` status dots and `▾` project markers live in column 0, so an
        // indented rule would leave them escaping to its left — unanchored.
        // Width = glyph gutter(2) + columns + two-space gaps + the 10-char
        // ALLOCATED date (wider than its label).
        let rule_width = 2 + w.name + 2 + w.port + 2 + 5 + 2 + w.env + 2 + 10;
        let rule = "─".repeat(rule_width);
        format!(
            "{}\n{}",
            self.palette.paint("1", &line),
            self.palette.paint("2", &rule)
        )
    }

    fn project(&self, name: &str, services: &HashMap<String, Entry>) -> String {
        let mut out = self.palette.paint("1;35", &format!("▾ {name}"));
        for (svc, entry) in sorted_services(services) {
            out.push('\n');
            out.push_str(&self.row(svc, entry));
        }
        out
    }

    fn row(&self, svc: &str, entry: &Entry) -> String {
        let (w, p) = (&self.widths, self.palette);
        let is_bound = self.bound.contains(&entry.port);
        let glyph = p.paint(
            if is_bound { "32" } else { "2" },
            port_status_glyph(is_bound),
        );

        // Pad on measured display width (icon counted) so columns align even
        // with double-width emoji; colour wraps the content, not the padding.
        let icon = match service_icon(svc) {
            Some(i) if p.active() => format!("{i} "),
            _ => String::new(),
        };
        let pad = " ".repeat(w.name.saturating_sub(name_cell_width(svc, p.active())));
        let name = format!("{icon}{}{pad}", p.paint("1", svc));

        let port = p.paint(
            "36",
            &format!("{:<pw$}", entry.port.to_string(), pw = w.port),
        );
        let proto = p.paint("2", &format!("{:<5}", entry.protocol.as_str()));
        let env = p.paint("33", &format!("{:<ew$}", env_var_name(svc), ew = w.env));
        let allocated = p.paint("2", &entry.allocated);
        format!("{glyph} {name}  {port}  {proto}  {env}  {allocated}")
    }
}

pub fn all_rich(reg: &Registry, bound: &HashSet<u16>, palette: &Palette) -> String {
    if reg.projects.is_empty() {
        return String::from("(no registrations)");
    }
    let entries: Vec<_> = reg.projects.values().flat_map(|s| s.iter()).collect();
    let view = RichView {
        bound,
        palette,
        widths: rich_widths(&entries, palette.active()),
    };
    let mut sorted: Vec<_> = reg.projects.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let blocks: Vec<String> = sorted
        .iter()
        .map(|(name, services)| view.project(name, services))
        .collect();
    // Header carries its own rule, so a single newline ties it to the first
    // project (the rule provides the visual separation, not a blank line).
    format!("{}\n{}", view.header(), blocks.join("\n\n"))
}

pub fn project_block_rich(
    project: &str,
    services: Option<&HashMap<String, Entry>>,
    bound: &HashSet<u16>,
    palette: &Palette,
) -> String {
    let populated = services.filter(|s| !s.is_empty());
    let entries: Vec<_> = populated.map(|s| s.iter().collect()).unwrap_or_default();
    let view = RichView {
        bound,
        palette,
        widths: rich_widths(&entries, palette.active()),
    };
    match populated {
        // Header carries its own rule (see `all_rich`); single newline.
        Some(s) => format!("{}\n{}", view.header(), view.project(project, s)),
        None => format!(
            "{}\n{}\n  (no registrations)",
            view.header(),
            palette.paint("1;35", &format!("▾ {project}"))
        ),
    }
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

    fn reg_with(services: &HashMap<String, Entry>) -> Registry {
        let mut reg = Registry::default();
        reg.projects
            .insert("github.com/acme/clawse".to_owned(), services.clone());
        reg
    }

    fn plain() -> Palette {
        Palette::new(false)
    }

    #[test]
    fn rich_header_names_every_column() {
        let out = all_rich(
            &reg_with(&one("postgres", 20_000)),
            &HashSet::new(),
            &plain(),
        );
        for col in ["SERVICE", "PORT", "PROTO", "ENV VAR", "ALLOCATED"] {
            assert!(out.contains(col), "header missing {col} in:\n{out}");
        }
    }

    #[test]
    fn rich_row_carries_env_var_and_date() {
        let out = all_rich(
            &reg_with(&one("postgres", 20_000)),
            &HashSet::new(),
            &plain(),
        );
        assert!(out.contains("POSTGRES_PORT"), "no env var in:\n{out}");
        assert!(out.contains("2026-04-21"), "no date in:\n{out}");
    }

    #[test]
    fn rich_active_colours_port_cyan() {
        let out = all_rich(
            &reg_with(&one("postgres", 20_000)),
            &HashSet::new(),
            &Palette::new(true),
        );
        assert!(
            out.contains("\x1b[36m20000\x1b[0m"),
            "port not cyan in:\n{out:?}"
        );
    }

    #[test]
    fn rich_bound_dot_is_green_free_dot_is_dim() {
        let bound: HashSet<u16> = [20_000].into_iter().collect();
        let p = Palette::new(true);
        let up = all_rich(&reg_with(&one("postgres", 20_000)), &bound, &p);
        let down = all_rich(&reg_with(&one("postgres", 20_000)), &HashSet::new(), &p);
        assert!(up.contains("\x1b[32m●\x1b[0m"), "bound not green: {up:?}");
        assert!(down.contains("\x1b[2m○\x1b[0m"), "free not dim: {down:?}");
    }

    #[test]
    fn rich_inactive_palette_emits_no_escapes() {
        let out = all_rich(
            &reg_with(&one("postgres", 20_000)),
            &HashSet::new(),
            &plain(),
        );
        assert!(
            !out.contains('\x1b'),
            "inactive rich leaked an escape:\n{out:?}"
        );
    }

    #[test]
    fn rich_columns_align_across_differing_name_lengths() {
        let mut services = HashMap::new();
        services.insert("db".to_owned(), entry(20_000, "2026-04-21"));
        services.insert("postgres".to_owned(), entry(20_001, "2026-04-21"));
        let out = all_rich(&reg_with(&services), &HashSet::new(), &plain());
        // Every data row's PORT column starts at the same offset.
        let offsets: Vec<usize> = out
            .lines()
            .filter(|l| l.contains("/tcp") || l.contains("20000") || l.contains("20001"))
            .filter_map(|l| l.find("2000"))
            .collect();
        assert!(offsets.len() >= 2, "expected two data rows in:\n{out}");
        assert!(
            offsets.windows(2).all(|w| w[0] == w[1]),
            "ports misaligned:\n{out}"
        );
    }

    #[test]
    fn rich_single_empty_project_is_descriptive() {
        let out = project_block_rich("proj", None, &HashSet::new(), &plain());
        assert!(out.contains("▾ proj"));
        assert!(out.contains("(no registrations)"));
    }
}
