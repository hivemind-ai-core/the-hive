//! Auth screen — informational only.
//!
//! Per-agent API keys are now set in the Agents panel (field: "API Key").
//! This screen can hold project-wide `.hive/.env` notes in the future.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::config::state::{ConfigWizardState, WizardCmd};

pub fn render(f: &mut Frame, area: Rect, _state: &ConfigWizardState) {
    let block = Block::default().title(" Auth ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(
            "API keys are configured per-agent in the Agents panel.\n\
             Select an agent → edit → API Key field.\n\n\
             Press l/→ to continue to Review, h/← to go back.",
        )
        .style(Style::default().fg(Color::DarkGray)),
        inner,
    );
}

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    match code {
        KeyCode::Char('l') | KeyCode::Right => state.go_next_screen(),
        KeyCode::Char('h') | KeyCode::Left  => state.go_prev_screen(),
        KeyCode::Char('q') | KeyCode::Esc   => return WizardCmd::Cancel,
        _ => {}
    }
    WizardCmd::Continue
}
