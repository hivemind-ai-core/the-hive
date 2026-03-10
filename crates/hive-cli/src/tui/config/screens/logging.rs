//! Logging config screen: log level selection.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::config::state::{ConfigWizardState, WizardCmd};

const LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default()
        .title(" Logging Configuration ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical(
        LEVELS
            .iter()
            .map(|_| Constraint::Length(1))
            .chain(std::iter::once(Constraint::Min(0)))
            .collect::<Vec<_>>(),
    )
    .split(inner);

    let current = &state.config.logging.level;
    for (i, &level) in LEVELS.iter().enumerate() {
        let selected = current.as_str() == level;
        let focused = state.field_idx == i;
        let style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if focused { "> " } else { "  " };
        let check = if selected { "[x] " } else { "[ ] " };
        let line = Line::from(vec![Span::styled(format!("{marker}{check}{level}"), style)]);
        f.render_widget(Paragraph::new(line), rows[i]);
    }
}

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    let max = LEVELS.len();
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.field_idx + 1 < max {
                state.field_idx += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.field_idx = state.field_idx.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(&level) = LEVELS.get(state.field_idx) {
                state.config.logging.level = level.to_string();
            }
        }
        KeyCode::Char('l') | KeyCode::Right => state.go_next_screen(),
        KeyCode::Char('h') | KeyCode::Left => state.go_prev_screen(),
        KeyCode::Char('q') | KeyCode::Esc => return WizardCmd::Cancel,
        _ => {}
    }
    WizardCmd::Continue
}
