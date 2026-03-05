//! Config wizard screen modules and shared rendering helpers.

pub mod agents;
pub mod app;
pub mod exec;
pub mod logging;
pub mod review;
pub mod server;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::state::{ConfigWizardState, WizardScreen};

/// Render the step indicator at the top of the wizard.
pub fn render_header(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let spans: Vec<Span> = WizardScreen::ALL
        .iter()
        .flat_map(|&s| {
            let style = if s == state.screen {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if s.index() < state.screen.index() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            [Span::styled(s.label().to_string(), style), Span::raw(" > ")]
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the keybinding hint at the bottom.
pub fn render_footer(f: &mut Frame, area: Rect, state: &ConfigWizardState) {
    let hint = if state.editing {
        "Enter:confirm  Esc:discard  Backspace:delete"
    } else {
        "j/k:move  Enter:edit  l/→:next  h/←:prev  q:cancel"
    };
    f.render_widget(Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)), area);
}

/// Render a single form field row.
///
/// `focused` — whether this field is currently selected.
/// `editing` — whether this field is being actively edited (shows `input`).
/// `display` — value to show when not editing.
/// `input` — current input buffer contents (used only when `editing`).
pub fn render_field<'a>(
    focused: bool,
    editing: bool,
    label: &'a str,
    display: &'a str,
    input: &'a str,
) -> Line<'a> {
    let (prefix_style, value_style) = if editing {
        (
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Yellow),
        )
    } else if focused {
        (
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::White),
        )
    } else {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::Gray),
        )
    };

    let marker = if focused && !editing { "> " } else { "  " };
    let value = if editing {
        format!("{input}▌")
    } else {
        display.to_string()
    };

    Line::from(vec![
        Span::styled(format!("{marker}{label:<22}", marker = marker, label = label), prefix_style),
        Span::styled(value, value_style),
    ])
}
