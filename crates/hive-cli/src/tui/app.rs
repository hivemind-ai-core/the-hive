//! TUI application state and main loop.

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::{Constraint, Direction, Layout}};

use super::events::{Action, next_action};
use super::poller;
use super::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Tasks,
    MessageBoard,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub state: AppState,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Dashboard,
            should_quit: false,
            state: AppState::default(),
        }
    }

    pub fn handle(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tab => {
                self.screen = match self.screen {
                    Screen::Dashboard => Screen::Tasks,
                    Screen::Tasks => Screen::MessageBoard,
                    Screen::MessageBoard => Screen::Dashboard,
                };
            }
            Action::Char('1') => self.screen = Screen::Dashboard,
            Action::Char('2') => self.screen = Screen::Tasks,
            Action::Char('3') => self.screen = Screen::MessageBoard,
            Action::Down => {
                if self.state.selected_task_idx + 1 < self.state.tasks.len() {
                    self.state.selected_task_idx += 1;
                }
            }
            Action::Up => {
                self.state.selected_task_idx = self.state.selected_task_idx.saturating_sub(1);
            }
            _ => {}
        }
    }
}

/// Run the TUI event loop. Connects to the hive-server at `server_url` for live updates.
pub fn run(server_url: String) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel::<poller::StateUpdate>();
    poller::spawn(server_url, tx);

    let mut app = App::new();
    let tick = Duration::from_millis(250);

    loop {
        // Drain all pending state updates from the poller.
        while let Ok(update) = rx.try_recv() {
            app.state.agents = update.agents;
            app.state.tasks = update.tasks.iter().map(|t| t.into()).collect();
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
                .split(f.area());

            super::dashboard::render_header(f, chunks[0], &app);
            match app.screen {
                Screen::Dashboard => super::dashboard::render(f, chunks[1], &app.state),
                Screen::Tasks => super::tasks_screen::render(f, chunks[1], &app.state),
                Screen::MessageBoard => super::message_board::render(f, chunks[1]),
            }
            super::dashboard::render_footer(f, chunks[2]);
        })?;

        let action = next_action(tick)?;
        app.handle(action);

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
