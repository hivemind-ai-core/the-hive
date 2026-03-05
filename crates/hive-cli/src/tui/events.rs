//! Terminal event handling.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

/// Simplified key actions understood by the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Quit,
    Tab,
    Char(char),
    None,
}

/// Poll for the next key event, non-blocking with a timeout.
pub fn next_action(timeout: Duration) -> Result<Action> {
    if !event::poll(timeout)? {
        return Ok(Action::None);
    }
    match event::read()? {
        Event::Key(KeyEvent { code, modifiers, .. }) => Ok(map_key(code, modifiers)),
        _ => Ok(Action::None),
    }
}

fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Action {
    // Ctrl-C → quit
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('h') | KeyCode::Left => Action::Left,
        KeyCode::Char('l') | KeyCode::Right => Action::Right,
        KeyCode::Enter => Action::Select,
        KeyCode::Esc => Action::Back,
        KeyCode::Tab => Action::Tab,
        KeyCode::Char(c) => Action::Char(c),
        _ => Action::None,
    }
}
