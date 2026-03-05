//! Message board screen: topic list + comment view.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::state::{AppState, TopicSummary};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_topic_list(f, chunks[0], state);
    render_topic_detail(f, chunks[1], state.topics.get(state.selected_topic_idx));
}

fn render_topic_detail(f: &mut Frame, area: Rect, topic: Option<&TopicSummary>) {
    let block = Block::default().title("Detail").borders(Borders::ALL);
    match topic {
        None => {
            f.render_widget(
                Paragraph::new("Select a topic\n\nControls:\n  j/k  navigate\n  n    new topic\n  c    comment")
                    .block(block),
                area,
            );
        }
        Some(t) => {
            let last = t.last_updated.as_deref().unwrap_or("-");
            let lines = vec![
                Line::from(vec![
                    Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&t.title),
                ]),
                Line::from(vec![
                    Span::styled("Comments: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(t.comment_count.to_string(), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(last),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "(Press Enter to fetch full topic and comments)",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
    }
}

fn render_topic_list(f: &mut Frame, area: Rect, state: &AppState) {
    if state.topics.is_empty() {
        f.render_widget(
            Paragraph::new("No topics")
                .block(Block::default().title("Topics").borders(Borders::ALL)),
            area,
        );
        return;
    }

    let selected = state.selected_topic_idx;

    let items: Vec<ListItem> = state
        .topics
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == selected {
                Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(&t.title),
                Span::styled(
                    format!(" [{}]", t.comment_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .style(style)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Topics").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)),
        area,
        &mut list_state,
    );
}
