//! TUI application state and main loop.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};

use super::events::{next_action, Action};
use super::poller::{self, TuiCmd};
use super::state::AppState;
use crate::config::Config;

/// Which field of the topic dialog is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicDialogField {
    Title,
    Content,
}

/// Which field of the task creation dialog is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDialogField {
    Title,
    Description,
    Tags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Tasks,
    MessageBoard,
    Agents,
    Settings,
}

/// Dialog for composing a push message.
pub struct PushDialog {
    pub target_agent_idx: usize,
    pub content: String,
}

/// Dialog for creating a new topic.
pub struct TopicDialog {
    pub title: String,
    pub content: String,
    pub active_field: TopicDialogField,
}

/// Dialog for adding a comment to a topic.
pub struct CommentDialog {
    pub topic_id: String,
    pub content: String,
}

/// Dialog for creating a new task.
pub struct TaskDialog {
    pub title: String,
    pub description: String,
    pub tags: String,
    pub active_field: TaskDialogField,
}

/// Dialog for editing an existing task.
pub struct TaskEditDialog {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tags: String,
    pub active_field: TaskDialogField,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub state: AppState,
    pub push_dialog: Option<PushDialog>,
    pub topic_dialog: Option<TopicDialog>,
    pub task_dialog: Option<TaskDialog>,
    pub comment_dialog: Option<CommentDialog>,
    pub task_edit_dialog: Option<TaskEditDialog>,
    pub cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<TuiCmd>>,
    pub project_dir: PathBuf,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Dashboard,
            should_quit: false,
            state: AppState::default(),
            push_dialog: None,
            topic_dialog: None,
            task_dialog: None,
            comment_dialog: None,
            task_edit_dialog: None,
            cmd_tx: None,
            project_dir: PathBuf::from("."),
        }
    }

    pub fn handle(&mut self, action: Action) {
        // Task edit dialog.
        if let Some(ref mut dialog) = self.task_edit_dialog {
            match action {
                Action::Back => {
                    self.task_edit_dialog = None;
                }
                Action::Tab => {
                    dialog.active_field = match dialog.active_field {
                        TaskDialogField::Title => TaskDialogField::Description,
                        TaskDialogField::Description => TaskDialogField::Tags,
                        TaskDialogField::Tags => TaskDialogField::Title,
                    };
                }
                Action::Char(c) => match dialog.active_field {
                    TaskDialogField::Title => dialog.title.push(c),
                    TaskDialogField::Description => dialog.description.push(c),
                    TaskDialogField::Tags => dialog.tags.push(c),
                },
                Action::Backspace => match dialog.active_field {
                    TaskDialogField::Title => {
                        dialog.title.pop();
                    }
                    TaskDialogField::Description => {
                        dialog.description.pop();
                    }
                    TaskDialogField::Tags => {
                        dialog.tags.pop();
                    }
                },
                Action::Select => {
                    if !dialog.title.trim().is_empty() {
                        if let Some(ref tx) = self.cmd_tx {
                            let tags: Vec<String> = dialog
                                .tags
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let _ = tx.send(TuiCmd::UpdateTask {
                                id: dialog.id.clone(),
                                title: dialog.title.clone(),
                                description: dialog.description.clone(),
                                tags,
                            });
                        }
                    }
                    self.task_edit_dialog = None;
                }
                _ => {}
            }
            return;
        }

        // Comment creation dialog.
        if let Some(ref mut dialog) = self.comment_dialog {
            match action {
                Action::Back => {
                    self.comment_dialog = None;
                }
                Action::Char(c) => {
                    dialog.content.push(c);
                }
                Action::Backspace => {
                    dialog.content.pop();
                }
                Action::Select => {
                    if !dialog.content.trim().is_empty() {
                        if let Some(ref tx) = self.cmd_tx {
                            let _ = tx.send(TuiCmd::CreateComment {
                                topic_id: dialog.topic_id.clone(),
                                content: dialog.content.clone(),
                            });
                        }
                    }
                    self.comment_dialog = None;
                }
                _ => {}
            }
            return;
        }

        // Task creation dialog.
        if let Some(ref mut dialog) = self.task_dialog {
            match action {
                Action::Back => {
                    self.task_dialog = None;
                }
                Action::Tab => {
                    dialog.active_field = match dialog.active_field {
                        TaskDialogField::Title => TaskDialogField::Description,
                        TaskDialogField::Description => TaskDialogField::Tags,
                        TaskDialogField::Tags => TaskDialogField::Title,
                    };
                }
                Action::Char(c) => match dialog.active_field {
                    TaskDialogField::Title => dialog.title.push(c),
                    TaskDialogField::Description => dialog.description.push(c),
                    TaskDialogField::Tags => dialog.tags.push(c),
                },
                Action::Backspace => match dialog.active_field {
                    TaskDialogField::Title => {
                        dialog.title.pop();
                    }
                    TaskDialogField::Description => {
                        dialog.description.pop();
                    }
                    TaskDialogField::Tags => {
                        dialog.tags.pop();
                    }
                },
                Action::Select => {
                    if !dialog.title.trim().is_empty() {
                        if let Some(ref tx) = self.cmd_tx {
                            let tags: Vec<String> = dialog
                                .tags
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let _ = tx.send(TuiCmd::CreateTask {
                                title: dialog.title.clone(),
                                description: dialog.description.clone(),
                                tags,
                            });
                        }
                    }
                    self.task_dialog = None;
                }
                _ => {}
            }
            return;
        }

        // Topic creation dialog.
        if let Some(ref mut dialog) = self.topic_dialog {
            match action {
                Action::Back => {
                    self.topic_dialog = None;
                }
                Action::Tab => {
                    dialog.active_field = match dialog.active_field {
                        TopicDialogField::Title => TopicDialogField::Content,
                        TopicDialogField::Content => TopicDialogField::Title,
                    };
                }
                Action::Char(c) => match dialog.active_field {
                    TopicDialogField::Title => dialog.title.push(c),
                    TopicDialogField::Content => dialog.content.push(c),
                },
                Action::Backspace => match dialog.active_field {
                    TopicDialogField::Title => {
                        dialog.title.pop();
                    }
                    TopicDialogField::Content => {
                        dialog.content.pop();
                    }
                },
                Action::Select => {
                    if !dialog.title.trim().is_empty() {
                        if let Some(ref tx) = self.cmd_tx {
                            let _ = tx.send(TuiCmd::CreateTopic {
                                title: dialog.title.clone(),
                                content: dialog.content.clone(),
                            });
                        }
                    }
                    self.topic_dialog = None;
                }
                _ => {}
            }
            return;
        }

        // Push message dialog.
        if let Some(ref mut dialog) = self.push_dialog {
            match action {
                Action::Back => {
                    self.push_dialog = None;
                }
                Action::Char(c) => {
                    dialog.content.push(c);
                }
                Action::Backspace => {
                    dialog.content.pop();
                }
                Action::Select => {
                    if let Some(agent) = self.state.agents.get(dialog.target_agent_idx) {
                        if let Some(ref tx) = self.cmd_tx {
                            let _ = tx.send(TuiCmd::SendPush {
                                to_agent_id: agent.id.clone(),
                                content: dialog.content.clone(),
                            });
                        }
                    }
                    self.push_dialog = None;
                }
                _ => {}
            }
            return;
        }

        match action {
            Action::Quit | Action::Char('q') | Action::Char('Q') => self.should_quit = true,
            Action::Tab => {
                self.screen = match self.screen {
                    Screen::Dashboard => Screen::Tasks,
                    Screen::Tasks => Screen::MessageBoard,
                    Screen::MessageBoard => Screen::Agents,
                    Screen::Agents => Screen::Settings,
                    Screen::Settings => Screen::Dashboard,
                };
            }
            Action::Char('1') => self.screen = Screen::Dashboard,
            Action::Char('2') => self.screen = Screen::Tasks,
            Action::Char('3') => self.screen = Screen::MessageBoard,
            Action::Char('4') => self.screen = Screen::Agents,
            Action::Char('5') => self.screen = Screen::Settings,
            Action::Char('s') if self.screen == Screen::Tasks => {
                if let Some(task) = self.state.tasks.get(self.state.selected_task_idx) {
                    // Cycle: pending → in-progress → done → pending
                    let next_status = match task.status.as_str() {
                        "pending" => "in-progress",
                        "in-progress" => "done",
                        "done" => "pending",
                        _ => "pending",
                    };
                    if let Some(ref tx) = self.cmd_tx {
                        let _ = tx.send(TuiCmd::SetTaskStatus {
                            id: task.id.clone(),
                            status: next_status.to_string(),
                        });
                    }
                }
            }
            Action::Char('r') if self.screen == Screen::Tasks => {
                if let Some(task) = self.state.tasks.get(self.state.selected_task_idx) {
                    if let Some(ref tx) = self.cmd_tx {
                        let _ = tx.send(TuiCmd::SetTaskStatus {
                            id: task.id.clone(),
                            status: "pending".to_string(),
                        });
                    }
                }
            }
            Action::Char('x') if self.screen == Screen::Tasks => {
                if let Some(task) = self.state.tasks.get(self.state.selected_task_idx) {
                    if let Some(ref tx) = self.cmd_tx {
                        let _ = tx.send(TuiCmd::SetTaskStatus {
                            id: task.id.clone(),
                            status: "cancelled".to_string(),
                        });
                    }
                }
            }
            Action::Char('e') if self.screen == Screen::Tasks => {
                if let Some(task) = self.state.tasks.get(self.state.selected_task_idx) {
                    self.task_edit_dialog = Some(TaskEditDialog {
                        id: task.id.clone(),
                        title: task.title.clone(),
                        description: task.description.clone().unwrap_or_default(),
                        tags: task.tags.join(", "),
                        active_field: TaskDialogField::Title,
                    });
                }
            }
            Action::Char('n') if self.screen == Screen::Tasks => {
                self.task_dialog = Some(TaskDialog {
                    title: String::new(),
                    description: String::new(),
                    tags: String::new(),
                    active_field: TaskDialogField::Title,
                });
            }
            Action::Char('n') if self.screen == Screen::MessageBoard => {
                self.topic_dialog = Some(TopicDialog {
                    title: String::new(),
                    content: String::new(),
                    active_field: TopicDialogField::Title,
                });
            }
            Action::Char('c') if self.screen == Screen::MessageBoard => {
                if let Some(topic) = self.state.topics.get(self.state.selected_topic_idx) {
                    self.comment_dialog = Some(CommentDialog {
                        topic_id: topic.id.clone(),
                        content: String::new(),
                    });
                }
            }
            Action::Select if self.screen == Screen::MessageBoard => {
                if let Some(topic) = self.state.topics.get(self.state.selected_topic_idx) {
                    if let Some(ref tx) = self.cmd_tx {
                        let _ = tx.send(TuiCmd::FetchTopic {
                            topic_id: topic.id.clone(),
                        });
                    }
                }
            }
            Action::Char('s') if self.screen == Screen::Settings => {
                let dir = self.project_dir.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let _ = rt.block_on(crate::commands::start(&dir));
                });
            }
            Action::Char('S') if self.screen == Screen::Settings => {
                let dir = self.project_dir.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let _ = rt.block_on(crate::commands::stop(&dir, false));
                });
            }
            Action::Char('r') if self.screen == Screen::Settings => {
                let dir = self.project_dir.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let _ = rt.block_on(crate::commands::restart(&dir));
                });
            }
            Action::Char('R') if self.screen == Screen::Settings => {
                let dir = self.project_dir.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let _ = rt.block_on(crate::commands::stop(&dir, true));
                });
            }
            Action::Char('p') if self.screen == Screen::Agents => {
                if !self.state.agents.is_empty() {
                    self.push_dialog = Some(PushDialog {
                        target_agent_idx: self.state.selected_agent_idx,
                        content: String::new(),
                    });
                }
            }
            Action::Char('[') if self.screen == Screen::Tasks => {
                self.state.task_detail_scroll = self.state.task_detail_scroll.saturating_sub(1);
            }
            Action::Char(']') if self.screen == Screen::Tasks => {
                self.state.task_detail_scroll = self.state.task_detail_scroll.saturating_add(1);
            }
            Action::Down => match self.screen {
                Screen::Tasks => {
                    if self.state.selected_task_idx + 1 < self.state.tasks.len() {
                        self.state.selected_task_idx += 1;
                        self.state.task_detail_scroll = 0;
                    }
                }
                Screen::MessageBoard => {
                    if self.state.selected_topic_idx + 1 < self.state.topics.len() {
                        self.state.selected_topic_idx += 1;
                    }
                }
                Screen::Agents => {
                    if self.state.selected_agent_idx + 1 < self.state.agents.len() {
                        self.state.selected_agent_idx += 1;
                    }
                }
                _ => {}
            },
            Action::Up => match self.screen {
                Screen::Tasks => {
                    let prev = self.state.selected_task_idx;
                    self.state.selected_task_idx = prev.saturating_sub(1);
                    if self.state.selected_task_idx != prev {
                        self.state.task_detail_scroll = 0;
                    }
                }
                Screen::MessageBoard => {
                    self.state.selected_topic_idx = self.state.selected_topic_idx.saturating_sub(1);
                }
                Screen::Agents => {
                    self.state.selected_agent_idx = self.state.selected_agent_idx.saturating_sub(1);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

fn render_task_fields(
    f: &mut Frame,
    area: Rect,
    title: &str,
    t: &str,
    desc: &str,
    tags: &str,
    active: &TaskDialogField,
) {
    let popup = centered_rect(65, 55, area);
    f.render_widget(Clear, popup);
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    for (i, (label, content, is_active)) in [
        ("Title", t, matches!(active, TaskDialogField::Title)),
        (
            "Description",
            desc,
            matches!(active, TaskDialogField::Description),
        ),
        (
            "Tags (comma-sep)",
            tags,
            matches!(active, TaskDialogField::Tags),
        ),
    ]
    .iter()
    .enumerate()
    {
        let style = if *is_active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        f.render_widget(
            Paragraph::new(content.to_string()).block(
                Block::default()
                    .title(*label)
                    .borders(Borders::ALL)
                    .style(style),
            ),
            rows[i],
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_two_field_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    label1: &str,
    val1: &str,
    label2: &str,
    val2: &str,
    first_active: bool,
) {
    let popup = centered_rect(65, 50, area);
    f.render_widget(Clear, popup);
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(inner);

    let active_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    f.render_widget(
        Paragraph::new(val1.to_string()).block(
            Block::default()
                .title(label1)
                .borders(Borders::ALL)
                .style(if first_active {
                    active_style
                } else {
                    Style::default()
                }),
        ),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(val2.to_string()).block(
            Block::default()
                .title(label2)
                .borders(Borders::ALL)
                .style(if !first_active {
                    active_style
                } else {
                    Style::default()
                }),
        ),
        rows[1],
    );
}

fn render_single_field_dialog(f: &mut Frame, area: Rect, title: &str, label: &str, val: &str) {
    let popup = centered_rect(65, 30, area);
    f.render_widget(Clear, popup);
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    f.render_widget(
        Paragraph::new(val.to_string()).block(
            Block::default().title(label).borders(Borders::ALL).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        inner,
    );
}

/// Run the TUI event loop. Connects to the hive-server at `server_url` for live updates.
#[allow(clippy::needless_pass_by_value)] // run() owns all its inputs for clarity at call sites
pub fn run(server_url: String, project_dir: PathBuf, config: Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel::<poller::StateUpdate>();
    let cmd_tx = poller::spawn(server_url, tx);

    let mut app = App::new();
    app.cmd_tx = Some(cmd_tx);
    app.project_dir = project_dir;
    let tick = Duration::from_millis(250);

    loop {
        // Drain all pending state updates from the poller.
        while let Ok(update) = rx.try_recv() {
            app.state.agents = update.agents;
            app.state.tasks = update.tasks.iter().map(|t| t.into()).collect();
            app.state.topics = update
                .topics
                .iter()
                .map(|t| super::state::TopicSummary {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    comment_count: 0,
                    last_updated: Some(t.last_updated_at.to_rfc3339()),
                })
                .collect();
            if update.topic_detail_id.is_some() {
                app.state.topic_detail_id = update.topic_detail_id;
                app.state.topic_comments = update.topic_comments;
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(f.area());

            super::dashboard::render_header(f, chunks[0], &app);
            match app.screen {
                Screen::Dashboard => super::dashboard::render(f, chunks[1], &app.state),
                Screen::Tasks => super::tasks_screen::render(f, chunks[1], &app.state),
                Screen::MessageBoard => super::message_board::render(f, chunks[1], &app.state),
                Screen::Agents => super::agents_screen::render(f, chunks[1], &app.state),
                Screen::Settings => super::settings_screen::render(f, chunks[1], Some(&config)),
            }

            // Render context-sensitive footer hint.
            let hint = if app.task_edit_dialog.is_some() || app.task_dialog.is_some() {
                "Tab:next field  Enter:save  Esc:cancel"
            } else if app.topic_dialog.is_some() {
                "Tab:next field  Enter:create  Esc:cancel"
            } else if app.comment_dialog.is_some() || app.push_dialog.is_some() {
                "Enter:send  Esc:cancel"
            } else {
                match app.screen {
                    Screen::Tasks => {
                        "n:new  e:edit  s:cycle  r:reset  x:cancel  ↑↓:select  []:scroll  q:quit"
                    }
                    Screen::MessageBoard => "n:new topic  c:comment  q:quit",
                    Screen::Agents => "p:push message  q:quit",
                    Screen::Settings => "s:start  S:stop  r:restart  R:reset  q:quit",
                    _ => "Tab:switch screens  1-5:go to screen  q:quit",
                }
            };
            f.render_widget(Paragraph::new(hint), chunks[2]);

            // Render active dialog overlays.
            let area = f.area();
            if let Some(ref d) = app.task_dialog {
                render_task_fields(
                    f,
                    area,
                    "New Task",
                    &d.title,
                    &d.description,
                    &d.tags,
                    &d.active_field,
                );
            } else if let Some(ref d) = app.task_edit_dialog {
                render_task_fields(
                    f,
                    area,
                    "Edit Task",
                    &d.title,
                    &d.description,
                    &d.tags,
                    &d.active_field,
                );
            } else if let Some(ref d) = app.topic_dialog {
                render_two_field_dialog(
                    f,
                    area,
                    "New Topic",
                    "Title",
                    &d.title,
                    "Content",
                    &d.content,
                    matches!(d.active_field, TopicDialogField::Title),
                );
            } else if let Some(ref d) = app.comment_dialog {
                render_single_field_dialog(f, area, "Add Comment", "Comment", &d.content);
            } else if let Some(ref d) = app.push_dialog {
                let target = app
                    .state
                    .agents
                    .get(d.target_agent_idx)
                    .map_or("?", |a| a.name.as_str());
                render_single_field_dialog(
                    f,
                    area,
                    &format!("Push → {target}"),
                    "Message",
                    &d.content,
                );
            }
        })?;

        let in_dialog = app.task_dialog.is_some()
            || app.task_edit_dialog.is_some()
            || app.topic_dialog.is_some()
            || app.comment_dialog.is_some()
            || app.push_dialog.is_some();
        let action = next_action(tick, in_dialog)?;
        app.handle(action);

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
