use std::time::Instant;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::campaign::{load_all_campaigns, queue_campaign, Campaign};
use crate::config::AppConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalStep {
    SelectRepo,
    EnterPrompt,
}

#[derive(Debug, Clone)]
pub struct ModalState {
    pub step: ModalStep,
    pub repos: Vec<String>,
    pub selected_repo_idx: usize,
    pub prompt_buffer: String,
    pub cursor_pos: usize,
}

impl ModalState {
    pub fn new(available_repos: Vec<String>) -> Self {
        Self {
            step: ModalStep::SelectRepo,
            repos: available_repos,
            selected_repo_idx: 0,
            prompt_buffer: "Audit domain logic, write reproduction tests first, fix edge cases, and execute PLAN.md iteratively".to_string(),
            cursor_pos: 0,
        }
    }

    pub fn selected_repo(&self) -> &str {
        if self.repos.is_empty() {
            "chess-repertoire-srs"
        } else {
            &self.repos[self.selected_repo_idx % self.repos.len()]
        }
    }
}

pub struct App {
    pub config: AppConfig,
    pub campaigns: Vec<Campaign>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub modal: Option<ModalState>,
    pub message: Option<(String, Instant)>,
    pub should_quit: bool,
    pub available_repos: Vec<String>,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let available_repos = Self::detect_repos();
        let mut app = Self {
            config,
            campaigns: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            modal: None,
            message: None,
            should_quit: false,
            available_repos,
        };
        app.refresh();
        app
    }

    pub fn detect_repos() -> Vec<String> {
        let projects_dir = std::path::Path::new("/home/ubuntu/github-projects");
        let mut repos = Vec::new();
        if projects_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(projects_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.join(".git").exists() {
                        repos.push(path.file_name().unwrap_or_default().to_string_lossy().to_string());
                    }
                }
            }
        }
        repos.sort();
        if let Some(pos) = repos.iter().position(|r| r == "chess-repertoire-srs") {
            let r = repos.remove(pos);
            repos.insert(0, r);
        }
        if repos.is_empty() {
            repos.push("chess-repertoire-srs".to_string());
        }
        repos
    }

    pub fn refresh(&mut self) {
        self.campaigns = load_all_campaigns(&self.config);
        if self.selected_index >= self.campaigns.len() && !self.campaigns.is_empty() {
            self.selected_index = self.campaigns.len() - 1;
        }
    }

    pub fn next(&mut self) {
        if !self.campaigns.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.campaigns.len() - 1);
        }
    }

    pub fn previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn selected_campaign(&self) -> Option<&Campaign> {
        self.campaigns.get(self.selected_index)
    }

    pub fn set_message(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    pub fn open_new_campaign_modal(&mut self) {
        let mut state = ModalState::new(self.available_repos.clone());
        state.cursor_pos = state.prompt_buffer.len();
        self.modal = Some(state);
    }

    pub fn close_modal(&mut self) {
        self.modal = None;
    }

    pub fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
        let mut should_queue = false;
        if let Some(ref mut modal) = self.modal {
            match modal.step {
                ModalStep::SelectRepo => match key.code {
                    KeyCode::Esc => {
                        self.modal = None;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if modal.selected_repo_idx > 0 {
                            modal.selected_repo_idx -= 1;
                        } else {
                            modal.selected_repo_idx = modal.repos.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !modal.repos.is_empty() {
                            modal.selected_repo_idx = (modal.selected_repo_idx + 1) % modal.repos.len();
                        }
                    }
                    KeyCode::Enter => {
                        modal.step = ModalStep::EnterPrompt;
                    }
                    _ => {}
                },
                ModalStep::EnterPrompt => match key.code {
                    KeyCode::Esc => {
                        self.modal = None;
                    }
                    KeyCode::Enter => {
                        should_queue = true;
                    }
                    KeyCode::Backspace => {
                        if modal.cursor_pos > 0 && modal.cursor_pos <= modal.prompt_buffer.len() {
                            modal.prompt_buffer.remove(modal.cursor_pos - 1);
                            modal.cursor_pos -= 1;
                        }
                    }
                    KeyCode::Delete => {
                        if modal.cursor_pos < modal.prompt_buffer.len() {
                            modal.prompt_buffer.remove(modal.cursor_pos);
                        }
                    }
                    KeyCode::Left => {
                        if modal.cursor_pos > 0 {
                            modal.cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right => {
                        if modal.cursor_pos < modal.prompt_buffer.len() {
                            modal.cursor_pos += 1;
                        }
                    }
                    KeyCode::Char(c) => {
                        if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
                            modal.prompt_buffer.insert(modal.cursor_pos, c);
                            modal.cursor_pos += 1;
                        }
                    }
                    _ => {}
                },
            }
        }

        if should_queue {
            if let Some(modal) = self.modal.take() {
                let repo = modal.selected_repo().to_string();
                let prompt = modal.prompt_buffer.trim().to_string();
                if !prompt.is_empty() {
                    match queue_campaign(&self.config, &repo, &prompt, 30, None) {
                        Ok(id) => {
                            self.set_message(format!("24/7 Campaign #{} queued for {}!", id, repo));
                            self.refresh();
                            self.selected_index = 0;
                        }
                        Err(e) => {
                            self.set_message(format!("Error queueing campaign: {}", e));
                        }
                    }
                }
            }
        }

        false
    }
}
