//! Message board screen: topic list + comment view.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub fn render(f: &mut Frame, area: Rect) {
    // Rendered with AppState in the next task iteration.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    // Topic list placeholder.
    let items: Vec<ListItem> = vec![
        ListItem::new("(no topics — connect to server)"),
    ];
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Topics").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)),
        chunks[0],
        &mut list_state,
    );

    // Comment/detail placeholder.
    f.render_widget(
        Paragraph::new("Select a topic to view comments\n\nControls:\n  j/k  navigate\n  Enter  open\n  n    new topic\n  c    comment")
            .block(Block::default().title("Comments").borders(Borders::ALL)),
        chunks[1],
    );
}
