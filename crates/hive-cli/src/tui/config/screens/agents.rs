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
//!   j/k       — move between fields
//!   Enter     — start/commit editing the focused field
//!   ←/→/Space — cycle select fields (`coding_agent`, auth, kilo provider)
//!   Esc       — leave edit mode, return to list

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::render_field;
use crate::config::Agent;
use crate::tui::config::state::{ConfigWizardState, WizardCmd};

const CODING_AGENTS: &[&str] = &["kilo", "claude"];
const AUTH_MODES: &[&str] = &["none", "synced", "api_key"];

/// Number of visible fields for the agent editor based on auth mode.
fn agent_field_count(agent: &Agent) -> usize {
    match agent.auth.as_str() {
        "none" => 4,    // name, coding_agent, tags, auth
        "synced" => 5,  // + sub-field (provider or status)
        "api_key" => 6, // + api_key + endpoint_url
        _ => 4,
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    if let Some(idx) = state.agent_edit {
        render_edit(f, area, state, idx);
    } else {
        render_list(f, area, state);
    }
}

fn render_list(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default()
        .title(" Agents — press 'a' to add, 'd' to delete ")
        .borders(Borders::ALL);
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
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
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
                format!(
                    "{marker}{:<20} {:<8} [{}]",
                    agent.name,
                    agent.coding_agent,
                    agent.tags.join(",")
                ),
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
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let add_marker = if add_focused { "> " } else { "  " };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{add_marker}[ Add agent ]"),
            add_style,
        ))),
        rows[agent_count],
    );
}

fn render_edit(f: &mut Frame, area: Rect, state: &ConfigWizardState, idx: usize) {
    let Some(agent) = state.config.agents.get(idx) else {
        return;
    };
    let title = format!(" Edit agent '{}' — Esc to return ", agent.name);
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let field_count = agent_field_count(agent);
    let row_constraints: Vec<Constraint> = std::iter::repeat_n(Constraint::Length(1), field_count)
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();
    let rows = Layout::vertical(row_constraints).split(inner);

    let tags_display = agent.tags.join(", ");

    // Field 0: Name
    f.render_widget(
        Paragraph::new(render_field(
            state.agent_subfield == 0,
            state.agent_subfield == 0 && state.editing,
            "Name",
            &agent.name,
            &state.input,
        )),
        rows[0],
    );

    // Field 1: Coding agent (select)
    f.render_widget(
        Paragraph::new(render_select_field(
            state.agent_subfield == 1,
            "Coding agent",
            &agent.coding_agent,
        )),
        rows[1],
    );

    // Field 2: Tags
    f.render_widget(
        Paragraph::new(render_field(
            state.agent_subfield == 2,
            state.agent_subfield == 2 && state.editing,
            "Tags (comma-separated)",
            &tags_display,
            &state.input,
        )),
        rows[2],
    );

    // Field 3: Auth mode (select)
    f.render_widget(
        Paragraph::new(render_select_field(
            state.agent_subfield == 3,
            "Auth",
            &agent.auth,
        )),
        rows[3],
    );

    // Field 4: Conditional sub-field (shown when auth != "none")
    if field_count >= 5 {
        let focused4 = state.agent_subfield == 4;
        match agent.auth.as_str() {
            "api_key" => {
                let key_display = agent_env_key_status(agent, "ANTHROPIC_API_KEY");
                f.render_widget(
                    Paragraph::new(render_field(
                        focused4,
                        focused4 && state.editing,
                        "API Key (ANTHROPIC_API_KEY)",
                        &key_display,
                        &state.input,
                    )),
                    rows[4],
                );
            }
            "synced" if agent.coding_agent == "kilo" => {
                let provider_display = kilo_provider_display(state, agent);
                f.render_widget(
                    Paragraph::new(render_select_field(
                        focused4,
                        "Kilo provider",
                        &provider_display,
                    )),
                    rows[4],
                );
            }
            "synced" => {
                // claude synced — show sync status, Enter/space re-reads
                let status = claude_auth_status(agent);
                let style = if focused4 {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let marker = if focused4 { "> " } else { "  " };
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!("{marker}  {:<22}{status}", "Claude auth"),
                        style,
                    ))),
                    rows[4],
                );
            }
            _ => {}
        }
    }

    // Field 5: Endpoint URL (api_key only)
    if field_count >= 6 {
        let focused5 = state.agent_subfield == 5;
        let endpoint_current = agent
            .env
            .get("ANTHROPIC_BASE_URL")
            .map_or("", String::as_str)
            .to_string();
        f.render_widget(
            Paragraph::new(render_field(
                focused5,
                focused5 && state.editing,
                "Endpoint URL (ANTHROPIC_BASE_URL)",
                &endpoint_current,
                &state.input,
            )),
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
            if state.field_idx + 1 < max {
                state.field_idx += 1;
            }
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
            KeyCode::Backspace => {
                state.input.pop();
            }
            KeyCode::Enter => commit_agent_field(state),
            KeyCode::Esc => state.stop_editing(),
            _ => {}
        }
        return WizardCmd::Continue;
    }

    let field_count = state
        .agent_edit
        .and_then(|i| state.config.agents.get(i))
        .map_or(4, agent_field_count);

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if state.agent_subfield + 1 < field_count {
                state.agent_subfield += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.agent_subfield = state.agent_subfield.saturating_sub(1);
        }

        // Field 1: cycle coding_agent
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
            if state.agent_subfield == 1 =>
        {
            cycle_coding_agent(state);
        }

        // Field 3: cycle auth mode
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
            if state.agent_subfield == 3 =>
        {
            cycle_auth_mode(state);
        }

        // Field 4: depends on auth mode
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
            if state.agent_subfield == 4 =>
        {
            let auth = state
                .agent_edit
                .and_then(|i| state.config.agents.get(i))
                .map(|a| a.auth.clone())
                .unwrap_or_default();
            let coding_agent = state
                .agent_edit
                .and_then(|i| state.config.agents.get(i))
                .map(|a| a.coding_agent.clone())
                .unwrap_or_default();
            match (auth.as_str(), coding_agent.as_str()) {
                ("api_key", _) => {
                    let val = current_agent_field_value(state);
                    state.start_editing(&val);
                }
                ("synced", "kilo") => {
                    if state.kilo_providers.is_empty() {
                        state.kilo_providers = load_kilo_providers();
                        state.kilo_provider_sel = 0;
                    } else {
                        state.kilo_provider_sel =
                            (state.kilo_provider_sel + 1) % state.kilo_providers.len().max(1);
                    }
                    write_kilo_provider_config(state);
                }
                ("synced", _) => {
                    // claude (or other) synced: re-read ~/.claude.json on Enter/space
                    sync_claude_auth(state);
                }
                _ => {}
            }
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
        name: format!("agent-{n}"),
        coding_agent: "kilo".to_string(),
        auth: "none".to_string(),
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
    let Some(idx) = state.agent_edit else {
        return String::new();
    };
    let Some(agent) = state.config.agents.get(idx) else {
        return String::new();
    };
    match state.agent_subfield {
        0 => agent.name.clone(),
        2 => agent.tags.join(", "),
        4 if agent.auth == "api_key" => String::new(), // API key always starts blank for security
        5 => agent
            .env
            .get("ANTHROPIC_BASE_URL")
            .cloned()
            .unwrap_or_default(),
        _ => String::new(), // select fields have no text value
    }
}

fn commit_agent_field(state: &mut ConfigWizardState) {
    let input = state.input.trim().to_string();
    let Some(idx) = state.agent_edit else {
        state.stop_editing();
        return;
    };
    if let Some(agent) = state.config.agents.get_mut(idx) {
        match state.agent_subfield {
            0 => agent.name = input,
            2 => {
                agent.tags = input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            4 if agent.auth == "api_key" => {
                if !input.is_empty() {
                    agent.env.insert("ANTHROPIC_API_KEY".to_string(), input);
                }
            }
            5 => {
                if input.is_empty() {
                    agent.env.remove("ANTHROPIC_BASE_URL");
                } else {
                    agent.env.insert("ANTHROPIC_BASE_URL".to_string(), input);
                }
            }
            _ => {}
        }
    }
    state.stop_editing();
}

/// Return true if the agent has auth credentials configured.
fn agent_has_auth(agent: &Agent) -> bool {
    match agent.auth.as_str() {
        "synced" => true,
        "api_key" => agent
            .env
            .get("ANTHROPIC_API_KEY")
            .is_some_and(|v| !v.is_empty()),
        _ => false,
    }
}

/// Return a display string indicating whether the API key is set in agent.env.
fn agent_env_key_status(agent: &Agent, key: &str) -> String {
    if agent.env.get(key).is_some_and(|v| !v.is_empty()) {
        "(set — Enter to update)".to_string()
    } else {
        "(not set — Enter to set)".to_string()
    }
}

/// Cycle the `coding_agent` value through `CODING_AGENTS` for the currently edited agent.
fn cycle_coding_agent(state: &mut ConfigWizardState) {
    let Some(idx) = state.agent_edit else { return };
    if let Some(agent) = state.config.agents.get_mut(idx) {
        let pos = CODING_AGENTS
            .iter()
            .position(|&s| s == agent.coding_agent)
            .unwrap_or(0);
        let next = (pos + 1) % CODING_AGENTS.len();
        agent.coding_agent = CODING_AGENTS[next].to_string();
    }
}

/// Cycle the auth mode through `AUTH_MODES` for the currently edited agent.
/// When transitioning to "synced" on a kilo agent, eagerly load providers.
fn cycle_auth_mode(state: &mut ConfigWizardState) {
    let Some(idx) = state.agent_edit else { return };
    let (new_auth, is_kilo) = if let Some(agent) = state.config.agents.get_mut(idx) {
        let pos = AUTH_MODES
            .iter()
            .position(|&s| s == agent.auth)
            .unwrap_or(0);
        let next = (pos + 1) % AUTH_MODES.len();
        agent.auth = AUTH_MODES[next].to_string();
        (agent.auth.clone(), agent.coding_agent == "kilo")
    } else {
        return;
    };
    // Eagerly load kilo providers / sync claude auth when switching to synced.
    if new_auth == "synced" {
        if is_kilo && state.kilo_providers.is_empty() {
            state.kilo_providers = load_kilo_providers();
            state.kilo_provider_sel = 0;
        } else if !is_kilo {
            sync_claude_auth(state);
        }
    }
    // Clamp subfield to new field count.
    let field_count = state
        .agent_edit
        .and_then(|i| state.config.agents.get(i))
        .map_or(4, agent_field_count);
    if state.agent_subfield >= field_count {
        state.agent_subfield = field_count - 1;
    }
}

/// Load provider IDs from ~/.kilocode/cli/config.json.
fn load_kilo_providers() -> Vec<String> {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/root"))
        .join(".kilocode/cli/config.json");
    let Ok(raw) = std::fs::read_to_string(&config_path) else {
        return vec![];
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return vec![];
    };
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
fn kilo_provider_display(state: &ConfigWizardState, agent: &Agent) -> String {
    // Check if a provider JSON is already stored in agent.env.
    if let Some(json_str) = agent.env.get("KILO_PROVIDER_JSON") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(id) = json["provider"].as_str() {
                return format!("{id} (synced)");
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

/// Store the selected kilo provider config JSON in `agent.env["KILO_PROVIDER_JSON"]`.
/// hive-agent will write this to ~/.kilocode/cli/config.json on startup.
fn write_kilo_provider_config(state: &mut ConfigWizardState) {
    let Some(idx) = state.agent_edit else { return };
    let provider_id = match state.kilo_providers.get(state.kilo_provider_sel) {
        Some(id) => id.clone(),
        None => return,
    };

    let src_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/root"))
        .join(".kilocode/cli/config.json");
    let Ok(raw) = std::fs::read_to_string(&src_path) else {
        return;
    };
    let Ok(src_json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let provider_obj = src_json["providers"]
        .as_array()
        .and_then(|arr| arr.iter().find(|p| p["id"].as_str() == Some(&provider_id)));
    let Some(mut provider) = provider_obj.cloned() else {
        return;
    };

    if let Some(obj) = provider.as_object_mut() {
        obj.insert(
            "id".to_string(),
            serde_json::Value::String("default".to_string()),
        );
    }

    let out = serde_json::json!({
        "providers": [provider],
        "provider": "default"
    });

    if let Some(agent) = state.config.agents.get_mut(idx) {
        agent.env.insert(
            "KILO_PROVIDER_JSON".to_string(),
            serde_json::to_string(&out).unwrap_or_default(),
        );
    }
}

/// Read `~/.claude.json` and store its contents in `agent.env["CLAUDE_AUTH_JSON"]`.
fn sync_claude_auth(state: &mut ConfigWizardState) {
    let Some(idx) = state.agent_edit else { return };
    let path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/root"))
        .join(".claude.json");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Some(agent) = state.config.agents.get_mut(idx) {
        agent.env.insert("CLAUDE_AUTH_JSON".to_string(), contents);
    }
}

/// Return a status string for the claude auth field.
fn claude_auth_status(agent: &Agent) -> &'static str {
    if agent
        .env
        .get("CLAUDE_AUTH_JSON")
        .is_some_and(|v| !v.is_empty())
    {
        "(synced from ~/.claude.json — Enter to re-sync)"
    } else {
        "(~/.claude.json not found — Enter to sync)"
    }
}

/// Render a select/cycle field (no text editing, shows ← value → arrows when focused).
fn render_select_field<'a>(focused: bool, label: &'a str, value: &'a str) -> Line<'a> {
    let (prefix_style, value_style) = if focused {
        (
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
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
