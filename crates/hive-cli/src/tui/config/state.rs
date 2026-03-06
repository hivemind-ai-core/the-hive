//! Wizard state shared across all config screens.

use std::path::PathBuf;

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardScreen {
    Server,
    Agents,
    App,
    Exec,
    Logging,
    Review,
}

impl WizardScreen {
    pub const ALL: &'static [WizardScreen] = &[
        WizardScreen::Server,
        WizardScreen::Agents,
        WizardScreen::App,
        WizardScreen::Exec,
        WizardScreen::Logging,
        WizardScreen::Review,
    ];

    pub fn next(self) -> Self {
        match self {
            Self::Server  => Self::Agents,
            Self::Agents  => Self::App,
            Self::App     => Self::Exec,
            Self::Exec    => Self::Logging,
            Self::Logging => Self::Review,
            Self::Review  => Self::Review,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Server  => Self::Server,
            Self::Agents  => Self::Server,
            Self::App     => Self::Agents,
            Self::Exec    => Self::App,
            Self::Logging => Self::Exec,
            Self::Review  => Self::Logging,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Server  => "Server",
            Self::Agents  => "Agents",
            Self::App     => "App",
            Self::Exec    => "Exec",
            Self::Logging => "Logging",
            Self::Review  => "Review",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Server  => 0,
            Self::Agents  => 1,
            Self::App     => 2,
            Self::Exec    => 3,
            Self::Logging => 4,
            Self::Review  => 5,
        }
    }
}

/// Return value from a screen's handle() function.
pub enum WizardCmd {
    /// Keep running the wizard.
    Continue,
    /// Save config and exit.
    Save,
    /// Discard changes and exit.
    Cancel,
}

pub struct ConfigWizardState {
    pub config: Config,
    pub screen: WizardScreen,
    /// Which item on the current screen is focused (0-indexed).
    pub field_idx: usize,
    /// Whether the focused field is being edited.
    pub editing: bool,
    /// Text input buffer used while editing.
    pub input: String,
    /// Agents screen: index of the agent currently being edited, or None for list mode.
    pub agent_edit: Option<usize>,
    /// Agents screen: which field within the agent edit form is focused.
    pub agent_subfield: usize,
    /// Project directory, available to screens for config path resolution.
    #[allow(dead_code)]
    pub project_dir: PathBuf,
    /// Kilo provider selection: available provider IDs loaded from ~/.kilocode/cli/config.json.
    pub kilo_providers: Vec<String>,
    /// Kilo provider selection: index of the currently highlighted provider.
    pub kilo_provider_sel: usize,
}

impl ConfigWizardState {
    pub fn new(config: Config, project_dir: PathBuf) -> Self {
        Self {
            config,
            screen: WizardScreen::Server,
            field_idx: 0,
            editing: false,
            input: String::new(),
            agent_edit: None,
            agent_subfield: 0,
            project_dir,
            kilo_providers: vec![],
            kilo_provider_sel: 0,
        }
    }

    pub fn go_next_screen(&mut self) {
        self.screen = self.screen.next();
        self.field_idx = 0;
        self.editing = false;
        self.input.clear();
    }

    pub fn go_prev_screen(&mut self) {
        self.screen = self.screen.prev();
        self.field_idx = 0;
        self.editing = false;
        self.input.clear();
    }

    pub fn start_editing(&mut self, current: &str) {
        self.editing = true;
        self.input = current.to_string();
    }

    pub fn stop_editing(&mut self) {
        self.editing = false;
        self.input.clear();
    }
}
