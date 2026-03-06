//! Agents screen: shows connected agents and their details.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::state::AppState;
use hive_core::types::Agent;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    if state.agents.is_empty() {
        f.render_widget(
            Paragraph::new("No agents connected")
                .block(Block::default().title("Agents").borders(Borders::ALL)),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let items: Vec<ListItem> = state
        .agents
        .iter()
        .map(|a| ListItem::new(Line::from(vec![Span::raw(&a.name)])))
        .collect();

    let selected = state.selected_agent_idx.min(state.agents.len().saturating_sub(1));
    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_stateful_widget(
        List::new(items).block(Block::default().title("Agents").borders(Borders::ALL)),
        chunks[0],
        &mut list_state,
    );

    render_detail(f, chunks[1], state.agents.get(selected));
}

fn render_detail(f: &mut Frame, area: Rect, agent: Option<&Agent>) {
    let block = Block::default().title("Detail").borders(Borders::ALL);
    match agent {
        None => f.render_widget(Paragraph::new("Select an agent").block(block), area),
        Some(a) => {
            let tags = if a.tags.is_empty() {
                "-".to_string()
            } else {
                a.tags.join(", ")
            };
            let last_seen = a
                .last_seen_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "-".to_string());
            let lines = vec![
                Line::from(vec![
                    Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&a.id),
                ]),
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&a.name),
                ]),
                Line::from(vec![
                    Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(tags, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Last seen: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(last_seen),
                ]),
            ];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
    }
}
