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
//!   j/k       — move between fields (name, coding_agent, tags)
//!   Enter     — start/commit editing the focused field
//!   ←/→/Space — cycle coding_agent select field (kilo/claude)
//!   Esc       — leave edit mode, return to list

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

const CODING_AGENTS: &[&str] = &["kilo", "claude"];

/// Fields common to all agents: name, coding_agent, tags, api_key, endpoint_url.
const AGENT_FIELDS_BASE: usize = 5;
/// Extra field for kilo agents: kilo_provider.
const AGENT_FIELDS_KILO: usize = 6;

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
        let base_style = if focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if focused { "> " } else { "  " };
        let auth_ok = agent_has_auth(agent);
        let (auth_label, auth_style) = if auth_ok {
            ("[auth]", Style::default().fg(Color::Green))
        } else {
            ("[!auth]", Style::default().fg(Color::Red))
        };
        let line = Line::from(vec![
            Span::styled(
                format!("{marker}{:<20} {:<8} [{}]", agent.name, agent.coding_agent, agent.tags.join(",")),
                base_style,
            ),
            Span::raw("  "),
            Span::styled(auth_label, auth_style),
        ]);
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

    let is_kilo = agent.coding_agent == "kilo";
    let row_constraints: Vec<Constraint> = std::iter::repeat(Constraint::Length(1))
        .take(if is_kilo { AGENT_FIELDS_KILO } else { AGENT_FIELDS_BASE })
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();
    let rows = Layout::vertical(row_constraints).split(inner);

    let tags_display = agent.tags.join(", ");

    // Field 0: Name (free text)
    let focused0 = state.agent_subfield == 0;
    f.render_widget(
        Paragraph::new(render_field(focused0, focused0 && state.editing, "Name", &agent.name, &state.input)),
        rows[0],
    );

    // Field 1: Coding agent (select widget — no freeform entry)
    let focused1 = state.agent_subfield == 1;
    f.render_widget(
        Paragraph::new(render_select_field(focused1, "Coding agent", &agent.coding_agent)),
        rows[1],
    );

    // Field 2: Tags (free text)
    let focused2 = state.agent_subfield == 2;
    f.render_widget(
        Paragraph::new(render_field(focused2, focused2 && state.editing, "Tags (comma-separated)", &tags_display, &state.input)),
        rows[2],
    );

    // Field 3: API Key (write-only, saved to agent.env)
    let focused3 = state.agent_subfield == 3;
    let key_label = format!("API Key ({})", api_key_name_for(&agent.coding_agent));
    let key_display = agent_env_key_status(agent, api_key_name_for(&agent.coding_agent));
    f.render_widget(
        Paragraph::new(render_field(focused3, focused3 && state.editing, &key_label, &key_display, &state.input)),
        rows[3],
    );

    // Field 4: Endpoint URL (optional, saved to agent.env)
    let focused4 = state.agent_subfield == 4;
    let endpoint_key = endpoint_key_name_for(&agent.coding_agent);
    let endpoint_label = format!("Endpoint URL ({})", endpoint_key);
    let endpoint_current = agent.env.get(endpoint_key).map(String::as_str).unwrap_or("").to_string();
    f.render_widget(
        Paragraph::new(render_field(focused4, focused4 && state.editing, &endpoint_label, &endpoint_current, &state.input)),
        rows[4],
    );

    // Field 5: Kilo provider (kilo agents only)
    if is_kilo {
        let focused5 = state.agent_subfield == 5;
        let provider_display = kilo_provider_display(state, agent);
        f.render_widget(
            Paragraph::new(render_select_field(focused5, "Kilo provider", &provider_display)),
            rows[5],
        );
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

    let agent_fields = match state.agent_edit
        .and_then(|i| state.config.agents.get(i))
        .map(|a| a.coding_agent.as_str())
    {
        Some("kilo") => AGENT_FIELDS_KILO,
        _ => AGENT_FIELDS_BASE,
    };

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.agent_subfield + 1 < agent_fields { state.agent_subfield += 1; }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.agent_subfield = state.agent_subfield.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
            if state.agent_subfield == 1 =>
        {
            // Cycle the coding_agent select field — no text editing.
            cycle_coding_agent(state);
        }
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
            if state.agent_subfield == 5 =>
        {
            // Cycle kilo provider; load providers from ~/.kilocode if not yet loaded.
            if state.kilo_providers.is_empty() {
                state.kilo_providers = load_kilo_providers();
                state.kilo_provider_sel = 0;
            } else {
                state.kilo_provider_sel =
                    (state.kilo_provider_sel + 1) % state.kilo_providers.len().max(1);
            }
            write_kilo_provider_config(state);
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
        3 => String::new(), // API key always starts blank for security
        4 => agent.env.get(endpoint_key_name_for(&agent.coding_agent)).cloned().unwrap_or_default(),
        5 => String::new(), // kilo provider is a select field
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
            // field 1 (coding_agent) is a select widget — handled by cycle_coding_agent
            2 => {
                agent.tags = input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            3 => {
                // Write API key to agent.env in config
                if !input.is_empty() {
                    agent.env.insert(api_key_name_for(&agent.coding_agent).to_string(), input);
                }
            }
            4 => {
                // Write endpoint URL to agent.env in config (blank = remove)
                let key = endpoint_key_name_for(&agent.coding_agent).to_string();
                if input.is_empty() {
                    agent.env.remove(&key);
                } else {
                    agent.env.insert(key, input);
                }
            }
            _ => {}
        }
    }
    state.stop_editing();
}

/// Return true if the agent has auth credentials configured in its env block.
fn agent_has_auth(agent: &crate::config::Agent) -> bool {
    let api_keys: &[&str] = &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GOOGLE_API_KEY"];
    api_keys.iter().any(|k| agent.env.get(*k).map(|v| !v.is_empty()).unwrap_or(false))
}

/// Return the env var name for an agent's primary API key.
fn api_key_name_for(coding_agent: &str) -> &'static str {
    match coding_agent {
        _ => "ANTHROPIC_API_KEY",
    }
}

/// Return the env var name for an agent's provider endpoint URL.
fn endpoint_key_name_for(coding_agent: &str) -> &'static str {
    match coding_agent {
        _ => "ANTHROPIC_BASE_URL",
    }
}

/// Return a display string indicating whether the API key is set in agent.env.
fn agent_env_key_status(agent: &crate::config::Agent, key: &str) -> String {
    if agent.env.get(key).map(|v| !v.is_empty()).unwrap_or(false) {
        "(set — Enter to update)".to_string()
    } else {
        "(not set — Enter to set)".to_string()
    }
}

/// Cycle the coding_agent value through CODING_AGENTS for the currently edited agent.
fn cycle_coding_agent(state: &mut ConfigWizardState) {
    let idx = match state.agent_edit { Some(i) => i, None => return };
    if let Some(agent) = state.config.agents.get_mut(idx) {
        let pos = CODING_AGENTS.iter().position(|&s| s == agent.coding_agent).unwrap_or(0);
        let next = (pos + 1) % CODING_AGENTS.len();
        agent.coding_agent = CODING_AGENTS[next].to_string();
    }
}

/// Load provider IDs from ~/.kilocode/cli/config.json.
fn load_kilo_providers() -> Vec<String> {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/root"))
        .join(".kilocode/cli/config.json");
    let Ok(raw) = std::fs::read_to_string(&config_path) else { return vec![] };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else { return vec![] };
    json["providers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Return a display string for the kilo provider field.
fn kilo_provider_display(state: &ConfigWizardState, agent: &crate::config::Agent) -> String {
    // Check if a per-agent kilo config already exists.
    let per_agent_dir = crate::config::io::hive_dir(&state.project_dir)
        .join(format!("kilocode-{}", agent.name));
    let config_path = per_agent_dir.join("cli/config.json");
    if config_path.exists() {
        if let Ok(raw) = std::fs::read_to_string(&config_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(id) = json["provider"].as_str() {
                    return format!("{id} (synced)");
                }
            }
        }
    }
    if state.kilo_providers.is_empty() {
        "(Enter to load from ~/.kilocode)".to_string()
    } else if let Some(id) = state.kilo_providers.get(state.kilo_provider_sel) {
        format!("{id} (not yet saved — Enter to confirm)")
    } else {
        "(no providers found in ~/.kilocode/cli/config.json)".to_string()
    }
}

/// Write a minimal per-agent kilocode config for the currently selected provider.
fn write_kilo_provider_config(state: &mut ConfigWizardState) {
    let idx = match state.agent_edit { Some(i) => i, None => return };
    let agent_name = match state.config.agents.get(idx) {
        Some(a) => a.name.clone(),
        None => return,
    };
    let provider_id = match state.kilo_providers.get(state.kilo_provider_sel) {
        Some(id) => id.clone(),
        None => return,
    };

    // Read source provider object from ~/.kilocode/cli/config.json.
    let src_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/root"))
        .join(".kilocode/cli/config.json");
    let Ok(raw) = std::fs::read_to_string(&src_path) else { return };
    let Ok(src_json) = serde_json::from_str::<serde_json::Value>(&raw) else { return };
    let provider_obj = src_json["providers"]
        .as_array()
        .and_then(|arr| arr.iter().find(|p| p["id"].as_str() == Some(&provider_id)));
    let Some(mut provider) = provider_obj.cloned() else { return };

    // Rename provider id to "default".
    if let Some(obj) = provider.as_object_mut() {
        obj.insert("id".to_string(), serde_json::Value::String("default".to_string()));
    }

    let out = serde_json::json!({
        "providers": [provider],
        "provider": "default"
    });

    let dst_dir = crate::config::io::hive_dir(&state.project_dir)
        .join(format!("kilocode-{agent_name}"))
        .join("cli");
    let _ = std::fs::create_dir_all(&dst_dir);
    let _ = std::fs::write(dst_dir.join("config.json"), serde_json::to_string_pretty(&out).unwrap_or_default());
}

/// Render a select/cycle field (no text editing, shows ← value → arrows).
fn render_select_field<'a>(focused: bool, label: &'a str, value: &'a str) -> Line<'a> {
    let (prefix_style, value_style) = if focused {
        (
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::White),
        )
    } else {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::Gray),
        )
    };
    let marker = if focused { "> " } else { "  " };
    let display = if focused {
        format!("← {value} →")
    } else {
        value.to_string()
    };
    Line::from(vec![
        Span::styled(format!("{marker}{label:<22}"), prefix_style),
        Span::styled(display, value_style),
    ])
}
