//! Rendering for `spout ls` and `spout whois`.
//!
//! `spout ls` has two layouts, split across submodules but sharing the
//! primitives defined here:
//!
//! - [`plain`] (`all` / `project_block`): the compact one-liner emitted
//!   whenever stdout is *not* an interactive terminal — pipes, redirects,
//!   `--no-tui`, `$NO_COLOR`. Scripts and agents depend on this byte-for-byte,
//!   so it is deliberately frozen.
//! - [`rich`] (`all_rich` / `project_block_rich`): the coloured, columned,
//!   icon-prefixed view a human sees in a real terminal. Replaces the old
//!   alternate-screen TUI — same information, printed inline.
//!
//! The choice between them is gated by [`should_colour`]; both agree on the
//! `●`/`○` convention via [`port_status_glyph`] and the same row order via
//! [`sorted_services`]. Submodules reach these through `super::` — child
//! modules can see an ancestor's private items, so nothing here needs to be
//! re-exported just for internal use.

use std::collections::HashMap;

use crate::registry::{Entry, HistoryEntry};

mod plain;
mod rich;

pub use plain::{all, project_block};
pub use rich::{all_rich, project_block_rich};

/// `●` = bound on OS, `○` = free. One source of truth for the glyph
/// convention, shared by every `ls` layout.
pub fn port_status_glyph(bound: bool) -> &'static str {
    if bound {
        "●"
    } else {
        "○"
    }
}

/// Whether an `ls` render should carry ANSI colour + icons. Pulled out as a
/// pure function so the truth table is testable without a real tty.
///
/// Colour only when stdout is an interactive terminal, `--no-tui` was not
/// passed, and `$NO_COLOR` is unset (the de-facto standard). Any of those
/// false ⇒ the plain, byte-stable layout.
pub fn should_colour(is_tty: bool, no_color: bool, no_tui: bool) -> bool {
    is_tty && !no_color && !no_tui
}

/// Carries the colour decision into the renderers. When inactive, `paint`
/// is a no-op — so the rich layout can be width-tested without escape noise,
/// and icons are suppressed (they are a terminal affordance only).
pub struct Palette {
    active: bool,
}

impl Palette {
    pub fn new(active: bool) -> Self {
        Self { active }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Wrap `s` in SGR `code` when active; one reset terminates it so colour
    /// never bleeds past the cell. Inactive ⇒ `s` unchanged. Private, but the
    /// `rich` submodule reaches it as a descendant.
    fn paint(&self, code: &str, s: &str) -> String {
        if self.active {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }
}

/// Sort services for display: by protocol then name, so TCP rows precede UDP
/// and the order is stable. Shared by both layouts so they never drift.
fn sorted_services(services: &HashMap<String, Entry>) -> Vec<(&String, &Entry)> {
    let mut sorted: Vec<_> = services.iter().collect();
    sorted.sort_by(|a, b| (a.1.protocol, a.0).cmp(&(b.1.protocol, b.0)));
    sorted
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

    #[test]
    fn should_colour_truth_table() {
        assert!(
            should_colour(true, false, false),
            "tty + colour + no flag → colour"
        );
        assert!(!should_colour(false, false, false), "piped → plain");
        assert!(!should_colour(true, true, false), "NO_COLOR → plain");
        assert!(!should_colour(true, false, true), "--no-tui → plain");
    }

    #[test]
    fn port_status_glyph_distinguishes_bound_from_free() {
        assert_eq!(port_status_glyph(true), "●");
        assert_eq!(port_status_glyph(false), "○");
    }
}
