//! Exec config screen: run_prefixes and command aliases.
//!
//! Fields (in order):
//!   0   — run_prefixes (comma-separated)
//!   1.. — one field per command alias sorted by key ("test = pnpm test")

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders},
};

use super::render_field;
use crate::tui::config::state::{ConfigWizardState, WizardCmd};

/// Sorted list of (key, value) pairs from the commands map.
fn sorted_commands(state: &ConfigWizardState) -> Vec<(String, String)> {
    let mut cmds: Vec<_> = state
        .config
        .exec
        .commands
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    cmds.sort_by(|a, b| a.0.cmp(&b.0));
    cmds
}

fn field_count(state: &ConfigWizardState) -> usize {
    1 + state.config.exec.commands.len()
}

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default().title(" Exec Configuration ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cmds = sorted_commands(state);
    let total_rows = 1 + cmds.len();
    let constraints: Vec<Constraint> = (0..total_rows)
        .map(|_| Constraint::Length(1))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let rows = Layout::vertical(constraints).split(inner);

    // Row 0: run_prefixes
    let prefixes_display = state.config.exec.run_prefixes.join(", ");
    let f0 = state.field_idx == 0;
    let e0 = f0 && state.editing;
    let line0 = render_field(f0, e0, "Run prefixes", &prefixes_display, &state.input);
    f.render_widget(ratatui::widgets::Paragraph::new(line0), rows[0]);

    // Rows 1..N: command aliases
    for (i, (key, val)) in cmds.iter().enumerate() {
        let label = format!("cmd: {key}");
        let focused = state.field_idx == i + 1;
        let editing = focused && state.editing;
        let line = render_field(focused, editing, &label, val, &state.input);
        f.render_widget(ratatui::widgets::Paragraph::new(line), rows[i + 1]);
    }
}

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    if state.editing {
        match code {
            KeyCode::Char(c) => state.input.push(c),
            KeyCode::Backspace => { state.input.pop(); }
            KeyCode::Enter => commit(state),
            KeyCode::Esc => state.stop_editing(),
            _ => {}
        }
        return WizardCmd::Continue;
    }

    let max = field_count(state);
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.field_idx + 1 < max { state.field_idx += 1; }
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
    if state.field_idx == 0 {
        return state.config.exec.run_prefixes.join(", ");
    }
    let cmds = sorted_commands(state);
    cmds.get(state.field_idx - 1)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn commit(state: &mut ConfigWizardState) {
    let input = state.input.trim().to_string();
    if state.field_idx == 0 {
        state.config.exec.run_prefixes = input
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        state.stop_editing();
        return;
    }
    let cmds = sorted_commands(state);
    if let Some((key, _)) = cmds.get(state.field_idx - 1) {
        state.config.exec.commands.insert(key.clone(), input);
    }
    state.stop_editing();
}
