//! Message board screen: topic list + comment view.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::state::AppState;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_topic_list(f, chunks[0], state);
    render_topic_detail(f, chunks[1], state);
}

fn render_topic_detail(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().title("Detail").borders(Borders::ALL);
    let topic = state.topics.get(state.selected_topic_idx);
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
            let loaded = state.topic_detail_id.as_deref() == Some(t.id.as_str());
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&t.title),
                ]),
                Line::from(vec![
                    Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(last),
                ]),
                Line::from(""),
            ];

            if loaded {
                lines.push(Line::from(Span::styled(
                    format!("Comments ({}):", state.topic_comments.len()),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                for comment in &state.topic_comments {
                    let from = comment.creator_agent_id.as_deref().unwrap_or("?");
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{from}] "), Style::default().fg(Color::Cyan)),
                        Span::raw(&comment.content),
                    ]));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    "(Press Enter to load comments)",
                    Style::default().fg(Color::DarkGray),
                )));
            }

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
