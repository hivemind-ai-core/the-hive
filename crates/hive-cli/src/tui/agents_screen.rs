//! Agents screen: shows connected agents and their details.

use chrono::Utc;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::state::{AppState, TaskSummary};
use hive_core::types::Agent;

fn staleness_secs(a: &Agent) -> Option<i64> {
    a.last_seen_at.map(|t| (Utc::now() - t).num_seconds())
}

fn current_task<'a>(tasks: &'a [TaskSummary], agent_id: &str) -> Option<&'a TaskSummary> {
    tasks
        .iter()
        .find(|t| t.status == "inprogress" && t.assigned.as_deref() == Some(agent_id))
}

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
        .map(|a| {
            let busy = current_task(&state.tasks, &a.id).is_some();
            let (status_marker, status_style) = if busy {
                ("● ", Style::default().fg(Color::Green))
            } else {
                ("○ ", Style::default().fg(Color::DarkGray))
            };
            let (name, name_style) = match staleness_secs(a) {
                None | Some(i64::MIN..=-1) => (
                    format!("{} (stale)", a.name),
                    Style::default().fg(Color::Red),
                ),
                Some(secs) if secs > 300 => (
                    format!("{} (stale)", a.name),
                    Style::default().fg(Color::Red),
                ),
                Some(secs) if secs > 60 => (
                    a.name.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::DIM),
                ),
                _ => (a.name.clone(), Style::default()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(status_marker, status_style),
                Span::styled(name, name_style),
            ]))
        })
        .collect();

    let selected = state
        .selected_agent_idx
        .min(state.agents.len().saturating_sub(1));
    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Agents").borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        chunks[0],
        &mut list_state,
    );

    let selected_agent = state.agents.get(selected);
    let selected_task = selected_agent.and_then(|a| current_task(&state.tasks, &a.id));
    render_detail(f, chunks[1], selected_agent, selected_task);
}

fn render_detail(f: &mut Frame, area: Rect, agent: Option<&Agent>, task: Option<&TaskSummary>) {
    let block = Block::default().title("Detail").borders(Borders::ALL);
    match agent {
        None => f.render_widget(Paragraph::new("Select an agent").block(block), area),
        Some(a) => {
            let tags = if a.tags.is_empty() {
                "-".to_string()
            } else {
                a.tags.join(", ")
            };
            let (last_seen, seen_style) = match staleness_secs(a) {
                None => ("-".to_string(), Style::default().fg(Color::Red)),
                Some(secs) if secs > 300 => (
                    format!("{}s ago (stale)", secs),
                    Style::default().fg(Color::Red),
                ),
                Some(secs) if secs > 60 => {
                    (format!("{}s ago", secs), Style::default().fg(Color::Yellow))
                }
                Some(secs) => (format!("{}s ago", secs), Style::default().fg(Color::Green)),
            };
            let (status_text, status_style) = match task {
                Some(t) => (
                    format!("● Working on: {}", t.title),
                    Style::default().fg(Color::Green),
                ),
                None => ("○ Idle".to_string(), Style::default().fg(Color::DarkGray)),
            };
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
                    Span::styled(last_seen, seen_style),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(status_text, status_style),
                ]),
            ];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
    }
}
