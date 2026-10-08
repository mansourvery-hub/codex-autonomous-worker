use std::time::Instant;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::campaign::{
    find_session_for_campaign, load_all_campaigns, queue_campaign,
    Campaign, CampaignStatus,
};
use crate::config::AppConfig;
use crate::prompts::PromptManager;
use crate::models::{get_codex_models, get_opencode_models, ModelInfo};
use crate::tui::pty::PtySession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    Sidebar,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalStep {
    SelectRepo,
    SelectEngine,
    SelectModel,
    SelectMode,
    EnterPrompt,
}

#[derive(Debug, Clone)]
pub struct EngineOption {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ModeOption {
    pub id: &'static str,
    pub label: &'static str,
    pub iterations: u32,
}

#[derive(Debug, Clone)]
pub struct ModalState {
    pub step: ModalStep,
    pub repos: Vec<String>,
    pub selected_repo_idx: usize,
    pub engines: Vec<EngineOption>,
    pub selected_engine_idx: usize,
    pub models: Vec<ModelOption>,
    pub selected_model_idx: usize,
    pub model_filter: String,
    pub modes: Vec<ModeOption>,
    pub selected_mode_idx: usize,
    pub prompt_buffer: String,
    pub cursor_pos: usize,
}

impl ModalState {
    pub fn new(available_repos: Vec<String>) -> Self {
        let engines = vec![
            EngineOption {
                id: "codex",
                label: "OpenAI Codex (Default autonomous worker)",
            },
            EngineOption {
                id: "opencode",
                label: "OpenCode v2 (Local agent)",
            },
        ];

        let modes = vec![
            ModeOption {
                id: "continuous",
                label: "⟳ 24/7 Continuous Loop (30 iterations, test-driven)",
                iterations: 30,
            },
            ModeOption {
                id: "single",
                label: "⊡ Single-Turn Task (1 turn quick fix)",
                iterations: 1,
            },
        ];

        // Initial default models from Codex
        let codex_models = get_codex_models();
        let models = codex_models
            .into_iter()
            .map(|m| ModelOption {
                id: m.id,
                label: m.display_name,
            })
            .collect();

        Self {
            step: ModalStep::SelectRepo,
            repos: available_repos,
            selected_repo_idx: 0,
            engines,
            selected_engine_idx: 0,
            models,
            selected_model_idx: 0,
            model_filter: String::new(),
            modes,
            selected_mode_idx: 0,
            prompt_buffer: PromptManager::default_task(&AppConfig::default()),
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

    pub fn selected_engine(&self) -> &EngineOption {
        &self.engines[self.selected_engine_idx % self.engines.len()]
    }

    pub fn selected_mode(&self) -> &ModeOption {
        &self.modes[self.selected_mode_idx % self.modes.len()]
    }

    pub fn filtered_models(&self) -> Vec<(usize, &ModelOption)> {
        if self.model_filter.trim().is_empty() {
            self.models.iter().enumerate().collect()
        } else {
            let query = self.model_filter.trim().to_lowercase();
            self.models
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.id.to_lowercase().contains(&query) || m.label.to_lowercase().contains(&query)
                })
                .collect()
        }
    }

    pub fn selected_model(&self) -> Option<&ModelOption> {
        let filtered = self.filtered_models();
        if filtered.is_empty() {
            None
        } else {
            let idx = self.selected_model_idx.min(filtered.len().saturating_sub(1));
            Some(filtered[idx].1)
        }
    }

    pub fn load_dynamic_models(&mut self) {
        let engine = self.selected_engine().id;
        let dynamic_list: Vec<ModelInfo> = if engine == "opencode" {
            get_opencode_models()
        } else {
            get_codex_models()
        };

        self.models = dynamic_list
            .into_iter()
            .map(|m| ModelOption {
                id: m.id,
                label: m.display_name,
            })
            .collect();
        self.selected_model_idx = 0;
        self.model_filter.clear();
    }
}

pub struct App {
    pub config: AppConfig,
    pub campaigns: Vec<Campaign>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub focused_pane: FocusedPane,
    pub pty: PtySession,
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
            focused_pane: FocusedPane::Sidebar,
            pty: PtySession::default(),
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

    pub fn next(&mut self, term_h: u16, term_w: u16) {
        if !self.campaigns.is_empty() {
            let prev_idx = self.selected_index;
            self.selected_index = (self.selected_index + 1) % self.campaigns.len();
            let visible_cards = (term_h.saturating_sub(4) / 4).max(1) as usize;
            if self.selected_index >= self.scroll_offset + visible_cards {
                self.scroll_offset = self.selected_index - visible_cards + 1;
            } else if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            }
            if self.selected_index != prev_idx {
                self.sync_pty_with_selection(term_h, term_w);
            }
        }
    }

    pub fn previous(&mut self, term_h: u16, term_w: u16) {
        if !self.campaigns.is_empty() {
            let prev_idx = self.selected_index;
            if self.selected_index > 0 {
                self.selected_index -= 1;
            } else {
                self.selected_index = self.campaigns.len() - 1;
            }
            let visible_cards = (term_h.saturating_sub(4) / 4).max(1) as usize;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            } else if self.selected_index >= self.scroll_offset + visible_cards {
                self.scroll_offset = self.selected_index - visible_cards + 1;
            }
            if self.selected_index != prev_idx {
                self.sync_pty_with_selection(term_h, term_w);
            }
        }
    }

    pub fn select_index(&mut self, idx: usize, term_h: u16, term_w: u16) {
        if idx < self.campaigns.len() {
            let prev_idx = self.selected_index;
            self.selected_index = idx;
            let visible_cards = (term_h.saturating_sub(4) / 4).max(1) as usize;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            } else if self.selected_index >= self.scroll_offset + visible_cards {
                self.scroll_offset = self.selected_index - visible_cards + 1;
            }
            if self.selected_index != prev_idx {
                self.sync_pty_with_selection(term_h, term_w);
            }
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

    pub fn cancel_selected_campaign(&mut self, term_h: u16, term_w: u16) {
        let campaign = match self.selected_campaign() {
            Some(c) => c.clone(),
            None => return,
        };

        if campaign.status != CampaignStatus::Running && campaign.status != CampaignStatus::Claimed {
            self.set_message(format!("Task #{} is not currently running.", campaign.id));
            return;
        }

        let _ = crate::campaign::cancel_campaign_by_id(&self.config, &campaign.id);

        self.set_message(format!("Stopped task #{}. Parallel slot freed.", campaign.id));
        self.pty.kill();
        self.refresh();
        self.sync_pty_with_selection(term_h, term_w);
    }

    pub fn sync_pty_with_selection(&mut self, term_rows: u16, term_cols: u16) {
        let campaign = match self.selected_campaign() {
            Some(c) => c.clone(),
            None => {
                self.pty.kill();
                return;
            }
        };

        if self.pty.active_task_id.as_deref() == Some(&campaign.id) && self.pty.is_running() {
            return;
        }

        let repo_dir = {
            let cand = std::path::PathBuf::from(format!("/home/ubuntu/github-projects/{}", campaign.repo));
            if cand.exists() {
                cand
            } else {
                self.config.repos_dir().join(&campaign.repo)
            }
        };

        if campaign.status == CampaignStatus::Running || campaign.status == CampaignStatus::Claimed {
            let task_session = format!("autopilot-{}", campaign.id);
            // Check if per-task session exists, else fallback to codex-live
            let has_task_sess = std::process::Command::new("tmux")
                .args(["has-session", "-t", &task_session])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            let attach_target = if has_task_sess { task_session } else { "codex-live".to_string() };

            let _ = self.pty.spawn(
                campaign.id.clone(),
                "tmux",
                &["attach", "-t", &attach_target],
                &repo_dir,
                term_rows,
                term_cols,
            );
        } else if campaign.status == CampaignStatus::Done || campaign.status == CampaignStatus::Failed {
            if campaign.agent == "opencode" {
                let _ = self.pty.spawn(
                    campaign.id.clone(),
                    "opencode",
                    &["--auto", "--continue", &repo_dir.to_string_lossy()],
                    &repo_dir,
                    term_rows,
                    term_cols,
                );
            } else if let Some(session_id) = find_session_for_campaign(&campaign.id) {
                let model_flag = format!("model={}", campaign.model);
                let _ = self.pty.spawn(
                    campaign.id.clone(),
                    "codex",
                    &[
                        "resume",
                        &session_id,
                        "-C",
                        &repo_dir.to_string_lossy(),
                        "-c",
                        "openai_base_url=http://127.0.0.1:8317/v1",
                        "-c",
                        "model_catalog_json=/home/ubuntu/.codex/model-catalogs/gateway.json",
                        "-c",
                        &model_flag,
                        "--dangerously-bypass-approvals-and-sandbox",
                    ],
                    &repo_dir,
                    term_rows,
                    term_cols,
                );
            } else {
                self.pty.kill();
            }
        } else {
            self.pty.kill();
        }
    }

    pub fn handle_modal_key(&mut self, key: KeyEvent, term_h: u16, term_w: u16) -> bool {
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
                        modal.step = ModalStep::SelectEngine;
                    }
                    _ => {}
                },
                ModalStep::SelectEngine => match key.code {
                    KeyCode::Esc => {
                        self.modal = None;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if modal.selected_engine_idx > 0 {
                            modal.selected_engine_idx -= 1;
                        } else {
                            modal.selected_engine_idx = modal.engines.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !modal.engines.is_empty() {
                            modal.selected_engine_idx = (modal.selected_engine_idx + 1) % modal.engines.len();
                        }
                    }
                    KeyCode::Enter => {
                        // Dynamically load models for the chosen engine
                        modal.load_dynamic_models();
                        modal.step = ModalStep::SelectModel;
                    }
                    _ => {}
                },
                ModalStep::SelectModel => {
                    let total = modal.filtered_models().len();
                    match key.code {
                        KeyCode::Esc => {
                            if !modal.model_filter.is_empty() {
                                modal.model_filter.clear();
                                modal.selected_model_idx = 0;
                            } else {
                                self.modal = None;
                            }
                        }
                        KeyCode::Up => {
                            if modal.selected_model_idx > 0 {
                                modal.selected_model_idx -= 1;
                            } else if total > 0 {
                                modal.selected_model_idx = total - 1;
                            }
                        }
                        KeyCode::Down => {
                            if total > 0 {
                                modal.selected_model_idx = (modal.selected_model_idx + 1) % total;
                            }
                        }
                        KeyCode::PageUp => {
                            modal.selected_model_idx = modal.selected_model_idx.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            if total > 0 {
                                modal.selected_model_idx = (modal.selected_model_idx + 10).min(total - 1);
                            }
                        }
                        KeyCode::Home => {
                            modal.selected_model_idx = 0;
                        }
                        KeyCode::End => {
                            if total > 0 {
                                modal.selected_model_idx = total - 1;
                            }
                        }
                        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if modal.selected_model_idx > 0 {
                                modal.selected_model_idx -= 1;
                            } else if total > 0 {
                                modal.selected_model_idx = total - 1;
                            }
                        }
                        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if total > 0 {
                                modal.selected_model_idx = (modal.selected_model_idx + 1) % total;
                            }
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            modal.model_filter.clear();
                            modal.selected_model_idx = 0;
                        }
                        KeyCode::Char(c) => {
                            modal.model_filter.push(c);
                            modal.selected_model_idx = 0;
                        }
                        KeyCode::Backspace => {
                            modal.model_filter.pop();
                            modal.selected_model_idx = 0;
                        }
                        KeyCode::Enter => {
                            if total > 0 {
                                modal.step = ModalStep::SelectMode;
                            }
                        }
                        _ => {}
                    }
                }
                ModalStep::SelectMode => match key.code {
                    KeyCode::Esc => {
                        self.modal = None;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if modal.selected_mode_idx > 0 {
                            modal.selected_mode_idx -= 1;
                        } else {
                            modal.selected_mode_idx = modal.modes.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !modal.modes.is_empty() {
                            modal.selected_mode_idx = (modal.selected_mode_idx + 1) % modal.modes.len();
                        }
                    }
                    KeyCode::Enter => {
                        if modal.selected_mode().id == "single" {
                            modal.prompt_buffer = "Inspect codebase and resolve open issue".to_string();
                            modal.cursor_pos = modal.prompt_buffer.len();
                        }
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
                let engine_opt = modal.selected_engine();
                let mode_opt = modal.selected_mode();
                let model_str = modal.selected_model().map(|m| m.id.as_str());
                let prompt = modal.prompt_buffer.trim().to_string();

                if !prompt.is_empty() {
                    match queue_campaign(
                        &self.config,
                        &repo,
                        &prompt,
                        mode_opt.id,
                        mode_opt.iterations,
                        model_str,
                        Some(engine_opt.id),
                    ) {
                        Ok(id) => {
                            self.set_message(format!(
                                "{} #{} queued with {} on {}! (Starting in parallel slot)",
                                if mode_opt.id == "continuous" { "24/7 Loop" } else { "Task" },
                                id,
                                engine_opt.label.split('(').next().unwrap_or(engine_opt.id).trim(),
                                repo
                            ));
                            self.refresh();
                            self.selected_index = 0;
                            self.sync_pty_with_selection(term_h, term_w);
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
