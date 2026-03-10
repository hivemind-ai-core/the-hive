//! Server config screen: port, `host_port`, `db_path`.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders},
    Frame,
};

use super::render_field;
use crate::tui::config::state::{ConfigWizardState, WizardCmd};

const FIELDS: usize = 3;

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default()
        .title(" Server Configuration ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cfg = &state.config.server;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let fields = [
        ("Port (container)", cfg.port.to_string()),
        ("Host port (exposed)", cfg.host_port.to_string()),
        ("DB path", cfg.db_path.clone()),
    ];

    for (i, (label, value)) in fields.iter().enumerate() {
        let focused = state.field_idx == i;
        let editing = focused && state.editing;
        let line = render_field(focused, editing, label, value, &state.input);
        f.render_widget(ratatui::widgets::Paragraph::new(line), rows[i]);
    }
}

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    if state.editing {
        match code {
            KeyCode::Char(c) => state.input.push(c),
            KeyCode::Backspace => {
                state.input.pop();
            }
            KeyCode::Enter => commit(state),
            KeyCode::Esc => state.stop_editing(),
            _ => {}
        }
        return WizardCmd::Continue;
    }

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.field_idx + 1 < FIELDS {
                state.field_idx += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.field_idx = state.field_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
            let val = current_value(state);
            state.start_editing(&val);
        }
        KeyCode::Char('l') | KeyCode::Right => state.go_next_screen(),
        KeyCode::Char('h') | KeyCode::Left => state.go_prev_screen(),
        KeyCode::Char('q') | KeyCode::Esc => return WizardCmd::Cancel,
        _ => {}
    }
    WizardCmd::Continue
}

fn current_value(state: &ConfigWizardState) -> String {
    let cfg = &state.config.server;
    match state.field_idx {
        0 => cfg.port.to_string(),
        1 => cfg.host_port.to_string(),
        2 => cfg.db_path.clone(),
        _ => String::new(),
    }
}

fn commit(state: &mut ConfigWizardState) {
    let input = state.input.trim().to_string();
    match state.field_idx {
        0 => {
            if let Ok(v) = input.parse::<u16>() {
                state.config.server.port = v;
            }
        }
        1 => {
            if let Ok(v) = input.parse::<u16>() {
                state.config.server.host_port = v;
            }
        }
        2 => {
            state.config.server.db_path = input;
        }
        _ => {}
    }
    state.stop_editing();
}
