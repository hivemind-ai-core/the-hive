//! Message board screen: topic list + comment view.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::state::AppState;
use super::util::strip_ansi;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_topic_list(f, chunks[0], state);
    render_topic_detail(f, chunks[1], state);
}

/// Resolve an agent ID to its display name, falling back to the raw ID.
fn resolve_agent_name<'a>(state: &'a AppState, agent_id: &'a str) -> &'a str {
    state
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .map(|a| a.name.as_str())
        .unwrap_or(agent_id)
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
            let creator = t
                .creator
                .as_deref()
                .map(|id| resolve_agent_name(state, id))
                .unwrap_or("-");
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&t.title),
                ]),
                Line::from(vec![
                    Span::styled("Creator: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(creator, Style::default().fg(Color::Cyan)),
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
                    let from = comment
                        .creator_agent_id
                        .as_deref()
                        .map(|id| resolve_agent_name(state, id))
                        .unwrap_or("?");
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{from}] "), Style::default().fg(Color::Cyan)),
                        Span::raw(strip_ansi(&comment.content)),
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
            let is_unread = state.unread_topic_ids.contains(&t.id);
            let style = if i == selected {
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut spans = vec![];
            if is_unread {
                spans.push(Span::styled("● ", Style::default().fg(Color::Yellow)));
            }
            spans.push(Span::raw(&t.title));
            spans.push(Span::styled(
                format!(" [{}]", t.comment_count),
                Style::default().fg(Color::DarkGray),
            ));
            if let Some(ref updater_id) = t.last_updated_by {
                let name = resolve_agent_name(state, updater_id);
                spans.push(Span::styled(
                    format!(" by {name}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Topics").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
        &mut list_state,
    );
}
