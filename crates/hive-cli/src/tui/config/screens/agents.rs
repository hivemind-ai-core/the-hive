//! Agents config screen: list management with add, edit, and delete.
//!
//! List mode: shows all agents + "Add agent" option.
//!   j/k   — move selection
//!   a     — add new agent (auto-named) and enter edit mode
//!   d     — delete selected agent
//!   Enter — edit selected agent
//!   l/→   — next screen
//!   h/←   — prev screen
//!   q     — cancel wizard
//!
//! Edit mode (for a specific agent):
//!   j/k   — move between fields (name, coding_agent, tags)
//!   Enter — start/commit editing the focused field
//!   Esc   — leave edit mode, return to list

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::render_field;
use crate::config::Agent;
use crate::tui::config::state::{ConfigWizardState, WizardCmd};

const AGENT_FIELDS: usize = 3; // name, coding_agent, tags

// ── Render ────────────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    if let Some(idx) = state.agent_edit {
        render_edit(f, area, state, idx);
    } else {
        render_list(f, area, state);
    }
}

fn render_list(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default().title(" Agents — press 'a' to add, 'd' to delete ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let agent_count = state.config.agents.len();
    let total_rows = agent_count + 1; // agents + "Add agent"
    let constraints: Vec<Constraint> = (0..total_rows)
        .map(|_| Constraint::Length(1))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();
    let rows = Layout::vertical(constraints).split(inner);

    for (i, agent) in state.config.agents.iter().enumerate() {
        let focused = state.field_idx == i;
        let style = if focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if focused { "> " } else { "  " };
        let line = Line::from(vec![Span::styled(
            format!("{marker}{:<20} {} [{}]", agent.name, agent.coding_agent, agent.tags.join(",")),
            style,
        )]);
        f.render_widget(Paragraph::new(line), rows[i]);
    }

    // "Add agent" row
    let add_focused = state.field_idx == agent_count;
    let add_style = if add_focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let add_marker = if add_focused { "> " } else { "  " };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("{add_marker}[ Add agent ]"), add_style))),
        rows[agent_count],
    );
}

fn render_edit(f: &mut Frame, area: Rect, state: &ConfigWizardState, idx: usize) {
    let agent = match state.config.agents.get(idx) {
        Some(a) => a,
        None => return,
    };
    let title = format!(" Edit agent '{}' — Esc to return ", agent.name);
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let tags_display = agent.tags.join(", ");
    let fields: [(&str, &str); 3] = [
        ("Name", &agent.name),
        ("Coding agent (kilo/claude)", &agent.coding_agent),
        ("Tags (comma-separated)", &tags_display),
    ];

    for (i, (label, value)) in fields.iter().enumerate() {
        let focused = state.agent_subfield == i;
        let editing = focused && state.editing;
        let line = render_field(focused, editing, label, value, &state.input);
        f.render_widget(Paragraph::new(line), rows[i]);
    }
}

// ── Handle ────────────────────────────────────────────────────────────────────

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    if state.agent_edit.is_some() {
        return handle_edit(code, state);
    }
    handle_list(code, state)
}

fn handle_list(code: KeyCode, state: &mut ConfigWizardState) -> WizardCmd {
    let agent_count = state.config.agents.len();
    let max = agent_count + 1; // include "Add agent" row

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.field_idx + 1 < max { state.field_idx += 1; }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.field_idx = state.field_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
            if state.field_idx == agent_count {
                add_agent(state);
            } else {
                state.agent_edit = Some(state.field_idx);
                state.agent_subfield = 0;
            }
        }
        KeyCode::Char('a') => add_agent(state),
        KeyCode::Char('d') => {
            if state.field_idx < agent_count {
                state.config.agents.remove(state.field_idx);
                if state.field_idx > 0 && state.field_idx >= state.config.agents.len() {
                    state.field_idx -= 1;
                }
            }
        }
        KeyCode::Char('l') | KeyCode::Right => state.go_next_screen(),
        KeyCode::Char('h') | KeyCode::Left => state.go_prev_screen(),
        KeyCode::Char('q') | KeyCode::Esc => return WizardCmd::Cancel,
        _ => {}
    }
    WizardCmd::Continue
}

fn handle_edit(code: KeyCode, state: &mut ConfigWizardState) -> WizardCmd {
    if state.editing {
        match code {
            KeyCode::Char(c) => state.input.push(c),
            KeyCode::Backspace => { state.input.pop(); }
            KeyCode::Enter => commit_agent_field(state),
            KeyCode::Esc => state.stop_editing(),
            _ => {}
        }
        return WizardCmd::Continue;
    }

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.agent_subfield + 1 < AGENT_FIELDS { state.agent_subfield += 1; }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.agent_subfield = state.agent_subfield.saturating_sub(1);
        }
        KeyCode::Enter => {
            let val = current_agent_field_value(state);
            state.start_editing(&val);
        }
        KeyCode::Esc => {
            state.agent_edit = None;
            state.agent_subfield = 0;
        }
        _ => {}
    }
    WizardCmd::Continue
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn add_agent(state: &mut ConfigWizardState) {
    let n = state.config.agents.len() + 1;
    let agent = Agent {
        name: format!("kilo-{n}"),
        coding_agent: "kilo".to_string(),
        tags: vec![],
        env: Default::default(),
    };
    state.config.agents.push(agent);
    let new_idx = state.config.agents.len() - 1;
    state.field_idx = new_idx;
    state.agent_edit = Some(new_idx);
    state.agent_subfield = 0;
}

fn current_agent_field_value(state: &ConfigWizardState) -> String {
    let idx = match state.agent_edit {
        Some(i) => i,
        None => return String::new(),
    };
    let agent = match state.config.agents.get(idx) {
        Some(a) => a,
        None => return String::new(),
    };
    match state.agent_subfield {
        0 => agent.name.clone(),
        1 => agent.coding_agent.clone(),
        2 => agent.tags.join(", "),
        _ => String::new(),
    }
}

fn commit_agent_field(state: &mut ConfigWizardState) {
    let input = state.input.trim().to_string();
    let idx = match state.agent_edit {
        Some(i) => i,
        None => { state.stop_editing(); return; }
    };
    if let Some(agent) = state.config.agents.get_mut(idx) {
        match state.agent_subfield {
            0 => agent.name = input,
            1 => {
                if input == "kilo" || input == "claude" {
                    agent.coding_agent = input;
                }
            }
            2 => {
                agent.tags = input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            _ => {}
        }
    }
    state.stop_editing();
}
