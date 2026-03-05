//! Review screen: shows a summary of all settings and confirms save or cancel.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::config::state::{ConfigWizardState, WizardCmd};

pub fn render(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let block = Block::default().title(" Review — press Enter to save ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cfg = &state.config;
    let kv_style = Style::default().fg(Color::White);
    let label_style = Style::default().fg(Color::DarkGray);

    let mut lines: Vec<Line> = Vec::new();

    let section = |title: &'static str| {
        Line::from(vec![Span::styled(
            format!("[{title}]"),
            Style::default().fg(Color::Yellow),
        )])
    };
    let field = |k: &str, v: String| {
        Line::from(vec![
            Span::styled(format!("  {k:<24}", k = k), label_style),
            Span::styled(v, kv_style),
        ])
    };

    lines.push(section("server"));
    lines.push(field("port", cfg.server.port.to_string()));
    lines.push(field("host_port", cfg.server.host_port.to_string()));
    lines.push(field("db_path", cfg.server.db_path.clone()));

    lines.push(Line::default());
    lines.push(section("agents"));
    if cfg.agents.is_empty() {
        lines.push(field("(none)", String::new()));
    }
    for agent in &cfg.agents {
        lines.push(field(&format!("  {}", agent.name), format!("{} [{}]", agent.coding_agent, agent.tags.join(","))));
    }

    lines.push(Line::default());
    lines.push(section("app"));
    lines.push(field("daemon_port", cfg.app.daemon_port.to_string()));
    lines.push(field("daemon_host_port", cfg.app.daemon_host_port.to_string()));

    lines.push(Line::default());
    lines.push(section("exec"));
    lines.push(field("run_prefixes", cfg.exec.run_prefixes.join(", ")));
    let mut cmds: Vec<_> = cfg.exec.commands.iter().collect();
    cmds.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in cmds {
        lines.push(field(&format!("  {k}"), v.clone()));
    }

    lines.push(Line::default());
    lines.push(section("logging"));
    lines.push(field("level", cfg.logging.level.clone()));

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  Enter: save    q/Esc: cancel    h/←: back",
        Style::default().fg(Color::Cyan),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}

pub fn handle(code: KeyCode, _mods: KeyModifiers, state: &mut ConfigWizardState) -> WizardCmd {
    match code {
        KeyCode::Enter | KeyCode::Char('s') => WizardCmd::Save,
        KeyCode::Char('h') | KeyCode::Left => {
            state.go_prev_screen();
            WizardCmd::Continue
        }
        KeyCode::Char('q') | KeyCode::Esc => WizardCmd::Cancel,
        _ => WizardCmd::Continue,
    }
}
