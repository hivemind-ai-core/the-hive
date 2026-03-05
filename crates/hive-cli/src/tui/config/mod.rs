//! Config TUI wizard — ratatui-based multi-screen config editor.
//!
//! Entry point: [`run_wizard`].

pub mod screens;
mod state;
mod wizard;

pub use state::WizardScreen;

use anyhow::Result;

use crate::config::Config;

/// Launch the interactive config wizard.
///
/// Returns the updated [`Config`] on save, or an error if the user cancels.
pub fn run_wizard(config: Config) -> Result<Config> {
    wizard::run(config)
}
