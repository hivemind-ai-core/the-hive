//! Settings screen: shows current configuration and docker management options.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::config::Config;

pub fn render(f: &mut Frame, area: Rect, config: Option<&Config>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(30),
            Constraint::Percentage(35),
        ])
        .split(area);

    render_config(f, chunks[0], config);
    render_api_keys(f, chunks[1]);
    render_docker_controls(f, chunks[2]);
}

fn render_config(f: &mut Frame, area: Rect, config: Option<&Config>) {
    let block = Block::default()
        .title("Configuration")
        .borders(Borders::ALL);
    match config {
        None => {
            f.render_widget(Paragraph::new("No configuration loaded").block(block), area);
        }
        Some(cfg) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        "Project ID:    ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(&cfg.project_id),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Server port:   ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(cfg.server.host_port.to_string()),
                ]),
                Line::from(vec![
                    Span::styled(
                        "DB path:       ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(&cfg.server.db_path),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Agents:        ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        cfg.agents
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Log level:     ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(&cfg.logging.level),
                ]),
            ];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
    }
}

fn key_status(var: &str) -> (String, Color) {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => (format!("set ({} chars)", v.len()), Color::Green),
        _ => ("not set".to_string(), Color::Red),
    }
}

fn render_api_keys(f: &mut Frame, area: Rect) {
    let keys = [
        ("ANTHROPIC_API_KEY", "Claude / Sonnet agents"),
        ("OPENAI_API_KEY", "OpenAI agents"),
        ("KILO_API_KEY", "Kilo coding agent"),
    ];
    let lines: Vec<Line> = keys
        .iter()
        .map(|(var, desc)| {
            let (status, color) = key_status(var);
            Line::from(vec![
                Span::styled(
                    format!("{var:<22}"),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{status:<18}"), Style::default().fg(color)),
                Span::styled(format!("# {desc}"), Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title("API Keys (from environment)")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_docker_controls(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Docker Controls",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("s  ", Style::default().fg(Color::Yellow)),
            Span::raw("start containers"),
        ]),
        Line::from(vec![
            Span::styled("S  ", Style::default().fg(Color::Yellow)),
            Span::raw("stop containers"),
        ]),
        Line::from(vec![
            Span::styled("r  ", Style::default().fg(Color::Yellow)),
            Span::raw("restart containers"),
        ]),
        Line::from(vec![
            Span::styled("R  ", Style::default().fg(Color::Red)),
            Span::raw("stop and remove containers"),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).block(Block::default().title("Docker").borders(Borders::ALL)),
        area,
    );
}
