//! Dashboard screen: agent status + task queue preview.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table},
};

use super::app::{App, Screen};
use super::state::AppState;

pub fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let tabs = [
        ("1:Dashboard", Screen::Dashboard),
        ("2:Tasks", Screen::Tasks),
        ("3:Board", Screen::MessageBoard),
    ];
    let spans: Vec<Span> = tabs
        .iter()
        .flat_map(|(label, screen)| {
            let style = if &app.screen == screen {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            [Span::styled(label.to_string(), style), Span::raw("  ")]
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_agents(f, cols[0], state);
    render_tasks(f, cols[1], state);
}

fn render_agents(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().title("Agents").borders(Borders::ALL);
    if state.agents.is_empty() {
        f.render_widget(Paragraph::new("No agents connected").block(block), area);
        return;
    }
    let rows: Vec<Row> = state
        .agents
        .iter()
        .map(|a| {
            let status_style = if a.connected_at.is_some() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Row::new(vec![a.name.clone(), "connected".to_string()])
                .style(status_style)
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Percentage(60), Constraint::Percentage(40)],
    )
    .header(Row::new(vec!["Agent", "Status"]).style(Style::default().add_modifier(Modifier::BOLD)))
    .block(block);
    f.render_widget(table, area);
}

fn render_tasks(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().title("Next Tasks").borders(Borders::ALL);
    if state.tasks.is_empty() {
        f.render_widget(Paragraph::new("No pending tasks").block(block), area);
        return;
    }
    let items: Vec<ListItem> = state
        .tasks
        .iter()
        .take(5)
        .map(|t| {
            let status_color = match t.status.as_str() {
                "in-progress" => Color::Yellow,
                "done" => Color::Green,
                "blocked" | "cancelled" => Color::Red,
                _ => Color::White,
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", t.status), Style::default().fg(status_color)),
                Span::raw(&t.title),
            ]))
        })
        .collect();
    f.render_widget(List::new(items).block(block), area);
}

pub fn render_footer(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new("Tab:switch  1/2/3:screen  q:quit  j/k:navigate"),
        area,
    );
}
