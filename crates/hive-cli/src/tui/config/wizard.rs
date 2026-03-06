//! Config wizard event loop.

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::{Constraint, Direction, Layout}};

use std::path::PathBuf;

use crate::config::Config;
use super::screens;
use super::state::{ConfigWizardState, WizardCmd, WizardScreen};

/// Run the config wizard. Returns the updated `Config` on save, or an error on cancel.
pub fn run(config: Config, project_dir: PathBuf) -> Result<Config> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = ConfigWizardState::new(config, project_dir);
    let tick = Duration::from_millis(250);

    let result = loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(f.area());

            screens::render_header(f, chunks[0], &state);
            match state.screen {
                WizardScreen::Server  => screens::server::render(f, chunks[1], &state),
                WizardScreen::Agents  => screens::agents::render(f, chunks[1], &state),
                WizardScreen::App     => screens::app::render(f, chunks[1], &state),
                WizardScreen::Exec    => screens::exec::render(f, chunks[1], &state),
                WizardScreen::Logging => screens::logging::render(f, chunks[1], &state),
                WizardScreen::Review  => screens::review::render(f, chunks[1], &state),
            }
            screens::render_footer(f, chunks[2], &state);
        })?;

        if !event::poll(tick)? {
            continue;
        }

        let ev = event::read()?;
        let Event::Key(KeyEvent { code, modifiers, .. }) = ev else { continue };

        // Global: Ctrl-C → cancel without save
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            break Err(anyhow::anyhow!("cancelled"));
        }

        let cmd = match state.screen {
            WizardScreen::Server  => screens::server::handle(code, modifiers, &mut state),
            WizardScreen::Agents  => screens::agents::handle(code, modifiers, &mut state),
            WizardScreen::App     => screens::app::handle(code, modifiers, &mut state),
            WizardScreen::Exec    => screens::exec::handle(code, modifiers, &mut state),
            WizardScreen::Logging => screens::logging::handle(code, modifiers, &mut state),
            WizardScreen::Review  => screens::review::handle(code, modifiers, &mut state),
        };

        match cmd {
            WizardCmd::Continue => {}
            WizardCmd::Save => break Ok(state.config),
            WizardCmd::Cancel => break Err(anyhow::anyhow!("cancelled")),
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}
