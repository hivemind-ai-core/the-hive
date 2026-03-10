//! Task list screen with filtering and detail view.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::state::{AppState, TaskSummary};
use super::util::strip_ansi;

fn status_color(status: &str) -> Color {
    match status {
        "in-progress" | "inprogress" => Color::Yellow,
        "done" => Color::Green,
        "blocked" | "cancelled" => Color::Red,
        _ => Color::White,
    }
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    if state.tasks.is_empty() {
        f.render_widget(
            Paragraph::new("No tasks").block(Block::default().title("Tasks").borders(Borders::ALL)),
            area,
        );
        return;
    }

    let selected = state.selected_task_idx;

    // Split: list on left, detail on right.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Task list.
    let items: Vec<ListItem> = state
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == selected {
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{}] ", &t.status[..std::cmp::min(4, t.status.len())]),
                    Style::default().fg(status_color(&t.status)),
                ),
                Span::raw(&t.title),
            ]))
            .style(style)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_stateful_widget(
        List::new(items).block(Block::default().title("Tasks").borders(Borders::ALL)),
        chunks[0],
        &mut list_state,
    );

    // Detail view.
    render_detail(
        f,
        chunks[1],
        state.tasks.get(selected),
        state.task_detail_scroll,
    );
}

fn render_detail(f: &mut Frame, area: Rect, task: Option<&TaskSummary>, scroll: u16) {
    let block = Block::default()
        .title("Detail ([ ] scroll)")
        .borders(Borders::ALL);
    match task {
        None => f.render_widget(Paragraph::new("Select a task").block(block), area),
        Some(t) => {
            let bold = Style::default().add_modifier(Modifier::BOLD);
            let mut lines = vec![
                Line::from(vec![Span::styled("ID: ", bold), Span::raw(&t.id)]),
                Line::from(vec![Span::styled("Title: ", bold), Span::raw(&t.title)]),
                Line::from(vec![
                    Span::styled("Status: ", bold),
                    Span::styled(&t.status, Style::default().fg(status_color(&t.status))),
                ]),
                Line::from(vec![
                    Span::styled("Assigned: ", bold),
                    Span::raw(t.assigned.as_deref().unwrap_or("-")),
                ]),
            ];
            if !t.tags.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Tags: ", bold),
                    Span::styled(t.tags.join(", "), Style::default().fg(Color::Cyan)),
                ]));
            }
            if let Some(ref desc) = t.description {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("Description:", bold)));
                for l in strip_ansi(desc).lines() {
                    lines.push(Line::from(Span::raw(l.to_string())));
                }
            }
            if let Some(ref result) = t.result {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("Result:", bold)));
                for l in strip_ansi(result).lines() {
                    lines.push(Line::from(Span::raw(l.to_string())));
                }
            }
            f.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                area,
            );
        }
    }
}
