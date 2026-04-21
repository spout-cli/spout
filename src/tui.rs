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

use std::collections::{HashMap, HashSet};
use std::io;

use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};

use crate::error::SpoutError;
use crate::format::port_status_glyph;
use crate::registry::{Entry, Registry};
use crate::services::{env_var_name, service_icon};

const COLUMN_WIDTHS: [Constraint; 4] = [
    Constraint::Length(28),
    Constraint::Length(7),
    Constraint::Length(24),
    Constraint::Min(12),
];

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

    let projects = sorted_projects(reg, project_filter);
    render_body(frame, layout[1], &projects, bound);

    let footer = Paragraph::new(Line::from(" ● bound   ○ free   [q/Esc] exit ").dim());
    frame.render_widget(footer, layout[2]);
}

fn render_body(
    frame: &mut Frame,
    area: Rect,
    projects: &[(&String, &HashMap<String, Entry>)],
    bound: &HashSet<u16>,
) {
    if projects.is_empty() {
        let msg = Paragraph::new(Line::from("  (no registrations)").dim());
        frame.render_widget(msg, area);
        return;
    }
    let sections = Layout::vertical(body_constraints(projects)).split(area);
    frame.render_widget(column_header_table(), sections[0]);

    let mut idx = 1;
    for (i, (project, services)) in projects.iter().enumerate() {
        frame.render_widget(project_title(project), sections[idx]);
        frame.render_widget(services_table(services, bound), sections[idx + 1]);
        idx += if i + 1 < projects.len() { 3 } else { 2 };
    }
}

fn body_constraints(projects: &[(&String, &HashMap<String, Entry>)]) -> Vec<Constraint> {
    let mut c = vec![Constraint::Length(2)];
    for (i, (_, services)) in projects.iter().enumerate() {
        c.push(Constraint::Length(1));
        c.push(Constraint::Length(services.len() as u16));
        if i + 1 < projects.len() {
            c.push(Constraint::Length(1));
        }
    }
    c
}

fn column_header_table() -> Table<'static> {
    let header = Row::new(vec![
        Cell::from("SERVICE").style(Style::new().bold()),
        Cell::from("PORT").style(Style::new().bold()),
        Cell::from("ENV VAR").style(Style::new().bold()),
        Cell::from("ALLOCATED").style(Style::new().bold()),
    ])
    .bottom_margin(1);
    Table::new(Vec::<Row>::new(), COLUMN_WIDTHS)
        .header(header)
        .column_spacing(2)
}

fn project_title<'a>(name: &'a str) -> Paragraph<'a> {
    Paragraph::new(Line::from(vec![
        Span::styled("▾ ", Style::new().fg(Color::Magenta)),
        Span::styled(name, Style::new().bold().fg(Color::Magenta)),
    ]))
}

fn services_table<'a>(services: &HashMap<String, Entry>, bound: &HashSet<u16>) -> Table<'a> {
    Table::new(collect_service_rows(services, bound), COLUMN_WIDTHS).column_spacing(2)
}

fn sorted_projects<'a>(
    reg: &'a Registry,
    project_filter: Option<&str>,
) -> Vec<(&'a String, &'a HashMap<String, Entry>)> {
    let mut projects: Vec<_> = match project_filter {
        None => reg.projects.iter().collect(),
        Some(name) => reg.projects.get_key_value(name).into_iter().collect(),
    };
    projects.sort_by(|a, b| a.0.cmp(b.0));
    projects
}

fn collect_service_rows(
    services: &HashMap<String, Entry>,
    bound: &HashSet<u16>,
) -> Vec<Row<'static>> {
    let mut svcs: Vec<_> = services.iter().collect();
    svcs.sort_by(|a, b| a.0.cmp(b.0));
    svcs.into_iter()
        .map(|(svc, entry)| {
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
            Row::new(vec![
                Cell::from(label),
                Cell::from(entry.port.to_string()).style(Style::new().fg(Color::Cyan)),
                Cell::from(env_var_name(svc)).style(Style::new().fg(Color::Yellow)),
                Cell::from(entry.allocated.clone()).style(Style::new().dim()),
            ])
        })
        .collect()
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
    fn sorted_projects_empty_registry_yields_no_entries() {
        let reg = Registry::default();
        assert!(sorted_projects(&reg, None).is_empty());
    }

    #[test]
    fn sorted_projects_filters_to_named_project() {
        let mut reg = Registry::default();
        insert(&mut reg, "a", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "b", "redis", 20_001, "2026-04-20");
        assert_eq!(sorted_projects(&reg, Some("a")).len(), 1);
        assert_eq!(sorted_projects(&reg, Some("b")).len(), 1);
        assert_eq!(sorted_projects(&reg, Some("nope")).len(), 0);
    }

    #[test]
    fn sorted_projects_returns_alphabetical_order() {
        let mut reg = Registry::default();
        insert(&mut reg, "zebra", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "apple", "redis", 20_001, "2026-04-20");
        let sorted = sorted_projects(&reg, None);
        assert_eq!(sorted[0].0, "apple");
        assert_eq!(sorted[1].0, "zebra");
    }

    #[test]
    fn collect_service_rows_one_row_per_service() {
        let mut reg = Registry::default();
        insert(&mut reg, "solo", "postgres", 20_000, "2026-04-20");
        insert(&mut reg, "solo", "redis", 20_001, "2026-04-20");
        let bound = HashSet::new();
        let services = reg.projects.get("solo").unwrap();
        assert_eq!(collect_service_rows(services, &bound).len(), 2);
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
