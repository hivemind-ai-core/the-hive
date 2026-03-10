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
    render_agent_auth(f, chunks[1], config);
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

fn render_agent_auth(f: &mut Frame, area: Rect, config: Option<&Config>) {
    let block = Block::default()
        .title("Agent Authentication")
        .borders(Borders::ALL);
    match config {
        None => {
            f.render_widget(Paragraph::new("No configuration loaded").block(block), area);
        }
        Some(cfg) if cfg.agents.is_empty() => {
            f.render_widget(Paragraph::new("No agents configured").block(block), area);
        }
        Some(cfg) => {
            let bold = Style::default().add_modifier(Modifier::BOLD);
            let lines: Vec<Line> = cfg
                .agents
                .iter()
                .map(|a| {
                    let auth_mode = if a.auth.is_empty() { "none" } else { &a.auth };
                    let (auth_label, auth_color) = match auth_mode {
                        "synced" => ("synced", Color::Green),
                        "api_key" => ("api_key", Color::Yellow),
                        "none" => ("none", Color::DarkGray),
                        other => (other, Color::Magenta),
                    };
                    Line::from(vec![
                        Span::styled(format!("{:<20}", a.name), bold),
                        Span::styled(format!("{:<12}", a.coding_agent), Style::default().fg(Color::Cyan)),
                        Span::styled(format!("auth: {auth_label}"), Style::default().fg(auth_color)),
                    ])
                })
                .collect();
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
    }
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
