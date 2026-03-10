//! Terminal event handling.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

/// Simplified key actions understood by the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Quit,
    Tab,
    Backspace,
    Char(char),
    None,
}

/// Poll for the next key event, non-blocking with a timeout.
/// When `text_mode` is true (e.g. a dialog is open), vi-keys h/j/k/l are
/// passed through as regular characters instead of navigation actions.
pub fn next_action(timeout: Duration, text_mode: bool) -> Result<Action> {
    if !event::poll(timeout)? {
        return Ok(Action::None);
    }
    match event::read()? {
        Event::Key(KeyEvent { code, modifiers, .. }) => Ok(map_key(code, modifiers, text_mode)),
        _ => Ok(Action::None),
    }
}

fn map_key(code: KeyCode, modifiers: KeyModifiers, text_mode: bool) -> Action {
    // Ctrl-C → quit
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    match code {
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,
        KeyCode::Char('k') if !text_mode => Action::Up,
        KeyCode::Char('j') if !text_mode => Action::Down,
        KeyCode::Char('h') if !text_mode => Action::Left,
        KeyCode::Char('l') if !text_mode => Action::Right,
        KeyCode::Enter => Action::Select,
        KeyCode::Esc => Action::Back,
        KeyCode::Tab => Action::Tab,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::Char(c),
        _ => Action::None,
    }
}
