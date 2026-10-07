pub mod app;
pub mod ui;

use std::io::{stdout, Stdout};
use std::process::Command;
use std::time::Duration;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use crate::campaign::{find_log_for_campaign, find_session_for_campaign, CampaignStatus};
use crate::config::AppConfig;
use self::app::App;

pub fn run_tui(config: AppConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config);
    let tick_rate = Duration::from_millis(250);

    let res = run_loop(&mut terminal, &mut app, tick_rate);

    // Clean terminal restoration
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    tick_rate: Duration,
) -> Result<()> {
    let mut last_refresh = std::time::Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    if app.modal.is_some() {
                        app.handle_modal_key(key);
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                app.should_quit = true;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                                app.should_quit = true;
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.next();
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.previous();
                            }
                            KeyCode::Char('r') => {
                                app.refresh();
                                app.set_message("Refreshed campaign queue.");
                            }
                            KeyCode::Char('n') => {
                                app.open_new_campaign_modal();
                            }
                            KeyCode::Enter => {
                                if let Some(campaign) = app.selected_campaign() {
                                    let status = campaign.status;
                                    let cid = campaign.id.clone();
                                    let repo = campaign.repo.clone();
                                    let model = campaign.model.clone();

                                    // Temporarily suspend terminal raw mode so Codex or pager owns the terminal
                                    disable_raw_mode()?;
                                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                                    connect_to_campaign(&app.config, &cid, &repo, &model, status)?;

                                    // Restore terminal raw mode for Ratatui
                                    enable_raw_mode()?;
                                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                                    terminal.clear()?;
                                    app.refresh();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse_event) => {
                    if app.modal.is_none() {
                        if let MouseEventKind::Down(_) = mouse_event.kind {
                            let click_y = mouse_event.row;
                            let start_y = 4;
                            let card_height = 4;
                            if click_y >= start_y {
                                let clicked_idx = ((click_y - start_y) / card_height) as usize;
                                if clicked_idx < app.campaigns.len() {
                                    app.selected_index = clicked_idx;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Auto refresh every 2 seconds
        if last_refresh.elapsed() >= Duration::from_secs(2) {
            app.refresh();
            last_refresh = std::time::Instant::now();
        }
    }

    Ok(())
}

fn connect_to_campaign(
    config: &AppConfig,
    task_id: &str,
    repo_name: &str,
    model: &str,
    status: CampaignStatus,
) -> Result<()> {
    let repo_dir = {
        let cand = std::path::PathBuf::from(format!("/home/ubuntu/github-projects/{}", repo_name));
        if cand.exists() {
            cand
        } else {
            config.repos_dir().join(repo_name)
        }
    };

    if status == CampaignStatus::Running || status == CampaignStatus::Claimed {
        println!("[32mConnecting directly to live Codex session...[0m");
        println!("[90m(Press Ctrl-b d to detach and return to Autopilot)[0m
");
        std::thread::sleep(Duration::from_millis(400));
        let _ = Command::new("tmux").args(["attach", "-t", "codex-live"]).status();
    } else if status == CampaignStatus::Done || status == CampaignStatus::Failed {
        if let Some(session_id) = find_session_for_campaign(task_id) {
            println!("[32mResuming recorded Codex session {}...[0m", session_id);
            println!("[90m(Exit session via /exit or Ctrl-C to return to Autopilot)[0m
");
            std::thread::sleep(Duration::from_millis(400));
            let _ = Command::new("codex")
                .args([
                    "resume",
                    &session_id,
                    "-C",
                    &repo_dir.to_string_lossy(),
                    "-c",
                    "openai_base_url=http://127.0.0.1:8317/v1",
                    "-c",
                    "model_catalog_json=/home/ubuntu/.codex/model-catalogs/gateway.json",
                    "-c",
                    &format!("model={}", model),
                    "--dangerously-bypass-approvals-and-sandbox",
                ])
                .status();
        } else if let Some(log_path) = find_log_for_campaign(config, task_id) {
            println!("[32mOpening execution log...[0m
");
            let _ = Command::new("less").args(["-R", "+G", &log_path.to_string_lossy()]).status();
        } else {
            println!("[33mNo recorded session or log found for campaign #{}[0m", task_id);
            std::thread::sleep(Duration::from_secs(1));
        }
    } else {
        println!("[33mCampaign #{} is currently queued. Waiting for supervisor daemon...[0m", task_id);
        std::thread::sleep(Duration::from_secs(1));
    }

    Ok(())
}
