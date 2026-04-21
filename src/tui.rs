//! Display-only TUI for `spout ls`.
//!
//! Rendered only when stdout is a TTY and `--no-tui` was not passed.
//! All Ratatui / crossterm imports live in this module — nothing else
//! in the crate touches them (see CODING_GUIDELINES.md §UI).
//!
//! Scope for Stage 2 is deliberately display-only: a styled table of
//! registered services, press q/Esc/Ctrl-C to exit. No navigation, no
//! clipboard, no drilldown. Keeps the module under the 400-line cap
//! and the UX promise tight ("see what's going on").

use std::collections::HashSet;
use std::io;

use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};

use crate::error::SpoutError;
use crate::format::port_status_glyph;
use crate::registry::Registry;
use crate::services::{env_var_name, service_icon};

/// Render the registry in a full-screen TUI. Blocks until the user exits.
/// `project_filter = Some(name)` shows only that project's services;
/// `None` shows everything grouped by project. `bound` is a snapshot of
/// which ports are currently bound on the OS — probed once by the caller
/// so the render loop stays cheap.
pub fn render(
    reg: &Registry,
    project_filter: Option<&str>,
    bound: &HashSet<u16>,
) -> Result<(), SpoutError> {
    let mut guard = TerminalGuard::new()?;
    loop {
        guard
            .terminal
            .draw(|f| draw(f, reg, project_filter, bound))
            .map_err(wrap("draw"))?;
        if poll_exit_key()? {
            return Ok(());
        }
    }
}

/// RAII guard for terminal state. `new()` puts the terminal into raw mode
/// on the alternate screen; `Drop` reverses both — even if the render loop
/// panics. Without this, a panic leaves the user's terminal in raw mode
/// with no cursor and their scrollback hidden.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self, SpoutError> {
        enable_raw_mode().map_err(wrap("enable raw mode"))?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).map_err(wrap("enter alternate screen"))?;
        let terminal =
            Terminal::new(CrosstermBackend::new(stdout)).map_err(wrap("terminal init"))?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(e) = disable_raw_mode() {
            tracing::warn!("failed to disable raw mode: {e}");
        }
        if let Err(e) = execute!(io::stdout(), LeaveAlternateScreen) {
            tracing::warn!("failed to leave alternate screen: {e}");
        }
    }
}

fn poll_exit_key() -> Result<bool, SpoutError> {
    match event::read().map_err(wrap("event read"))? {
        Event::Key(key) if key.kind == KeyEventKind::Press && is_exit_key(key) => Ok(true),
        _ => Ok(false),
    }
}

fn is_exit_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn wrap(op: &'static str) -> impl Fn(io::Error) -> SpoutError {
    move |e| SpoutError::RegistryCorrupt(format!("tui {op}: {e}"))
}

fn draw(frame: &mut Frame, reg: &Registry, project_filter: Option<&str>, bound: &HashSet<u16>) {
    let layout = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    let title_text = project_filter
        .map(str::to_owned)
        .unwrap_or_else(|| "all projects".to_owned());
    let title = Paragraph::new(Line::from(format!(" 💧 spout — {title_text} ")).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, layout[0]);

    let rows = collect_rows(reg, project_filter, bound);
    let widths = [
        Constraint::Length(26),
        Constraint::Length(7),
        Constraint::Length(20),
        Constraint::Length(12),
        Constraint::Min(20),
    ];
    let header = Row::new(vec![
        Cell::from("SERVICE").style(Style::new().bold()),
        Cell::from("PORT").style(Style::new().bold()),
        Cell::from("ENV VAR").style(Style::new().bold()),
        Cell::from("ALLOCATED").style(Style::new().bold()),
        Cell::from("PROJECT").style(Style::new().bold()),
    ])
    .bottom_margin(1);
    let table = Table::new(rows, widths).header(header).column_spacing(2);
    frame.render_widget(table, layout[1]);

    let footer = Paragraph::new(Line::from(" ● bound   ○ free   [q/Esc] exit ").dim());
    frame.render_widget(footer, layout[2]);
}

fn collect_rows(
    reg: &Registry,
    project_filter: Option<&str>,
    bound: &HashSet<u16>,
) -> Vec<Row<'static>> {
    let mut projects: Vec<_> = match project_filter {
        None => reg.projects.iter().collect(),
        Some(name) => reg.projects.get_key_value(name).into_iter().collect(),
    };
    projects.sort_by(|a, b| a.0.cmp(b.0));

    let mut rows = Vec::new();
    for (project, services) in projects {
        let mut svcs: Vec<_> = services.iter().collect();
        svcs.sort_by(|a, b| a.0.cmp(b.0));
        for (svc, entry) in svcs {
            let is_bound = bound.contains(&entry.port);
            let status_style = if is_bound {
                Style::new().fg(Color::Green)
            } else {
                Style::new().dim()
            };
            let name = match service_icon(svc) {
                Some(icon) => format!("{icon} {svc}"),
                None => svc.clone(),
            };
            let label = Line::from(vec![
                Span::styled(port_status_glyph(is_bound), status_style),
                Span::raw(" "),
                Span::styled(name, Style::new().bold()),
            ]);
            rows.push(Row::new(vec![
                Cell::from(label),
                Cell::from(entry.port.to_string()).style(Style::new().fg(Color::Cyan)),
                Cell::from(env_var_name(svc)).style(Style::new().fg(Color::Yellow)),
                Cell::from(entry.allocated.clone()).style(Style::new().dim()),
                Cell::from(project.clone()).style(Style::new().dim()),
            ]));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Entry;

    fn insert(reg: &mut Registry, project: &str, service: &str, port: u16, allocated: &str) {
        reg.projects.entry(project.to_owned()).or_default().insert(
            service.to_owned(),
            Entry {
                port,
                allocated: allocated.to_owned(),
            },
        );
    }

    #[test]
    fn collect_rows_empty_registry_yields_no_rows() {
        let reg = Registry::default();
        let bound = HashSet::new();
        assert!(collect_rows(&reg, None, &bound).is_empty());
    }

    #[test]
    fn collect_rows_filters_to_named_project() {
        let mut reg = Registry::default();
        insert(&mut reg, "a", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "b", "redis", 20_001, "2026-04-20");
        let bound = HashSet::new();
        assert_eq!(collect_rows(&reg, Some("a"), &bound).len(), 1);
        assert_eq!(collect_rows(&reg, Some("b"), &bound).len(), 1);
        assert_eq!(collect_rows(&reg, Some("nope"), &bound).len(), 0);
    }

    #[test]
    fn collect_rows_has_one_row_per_service_across_projects() {
        let mut reg = Registry::default();
        insert(&mut reg, "a", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "b", "redis", 20_001, "2026-04-20");
        let bound = HashSet::new();
        // One row per service, no separator rows — project is a column.
        assert_eq!(collect_rows(&reg, None, &bound).len(), 2);
    }

    #[test]
    fn collect_rows_single_project_is_one_row_per_service() {
        let mut reg = Registry::default();
        insert(&mut reg, "solo", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "solo", "redis", 20_001, "2026-04-20");
        let bound = HashSet::new();
        assert_eq!(collect_rows(&reg, None, &bound).len(), 2);
    }

    #[test]
    fn is_exit_key_recognises_q_esc_ctrl_c() {
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty());
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(is_exit_key(q));
        assert!(is_exit_key(esc));
        assert!(is_exit_key(ctrl_c));
    }

    #[test]
    fn is_exit_key_rejects_other_keys() {
        let a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty());
        let c_no_ctrl = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty());
        assert!(!is_exit_key(a));
        assert!(!is_exit_key(c_no_ctrl));
    }
}
