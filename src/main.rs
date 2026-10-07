use std::process::Command;
use anyhow::Result;
use clap::Parser;
use autopilot::cli::{Cli, Commands};
use autopilot::config::AppConfig;
use autopilot::campaign::{load_all_campaigns, load_current, queue_campaign, CampaignStatus};
use autopilot::supervisor::Supervisor;
use autopilot::tui::run_tui;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load();

    match cli.command {
        None | Some(Commands::Tui) => {
            run_tui(config)?;
        }
        Some(Commands::Loop { prompt, repo, model, iterations }) => {
            let prompt_text = prompt.join(" ").trim().to_string();
            let id = queue_campaign(&config, &repo, &prompt_text, iterations, model.as_deref())?;
            println!("[32m✔ Launched 24/7 Autonomous Campaign #{} for {}[0m", id, repo);
            println!("  Objective: {}", prompt_text);
            println!("  Budget:    {} iterations", iterations);
            println!("Opening Autopilot Control Deck...
");
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            run_tui(config)?;
        }
        Some(Commands::Queue { prompt, repo, model, iterations }) => {
            let prompt_text = prompt.join(" ").trim().to_string();
            if prompt_text.is_empty() {
                eprintln!("Error: Objective cannot be empty.");
                std::process::exit(1);
            }
            let repo_name = repo.unwrap_or_else(|| "chess-repertoire-srs".to_string());
            let id = queue_campaign(&config, &repo_name, &prompt_text, iterations, model.as_deref())?;
            println!("[32m✔ Autonomous campaign #{} queued for {}:[0m", id, repo_name);
            println!("  Objective: {}", prompt_text);
            println!("  Budget:    {} iterations", iterations);
            println!("
Launch 'autopilot' to open the control deck.");
        }
        Some(Commands::List) => {
            println!("[1m=== Active & Queued Autonomous Campaigns ===[0m");
            let campaigns = load_all_campaigns(&config);
            if campaigns.is_empty() {
                println!("  (Queue is empty)");
            }
            for c in campaigns {
                let (color, pill) = match c.status {
                    CampaignStatus::Running => ("[32m", "[WORKING]"),
                    CampaignStatus::Claimed => ("[33m", "[CLAIMED]"),
                    CampaignStatus::Pending => ("[36m", "[QUEUED ]"),
                    CampaignStatus::Done => ("[32m", "[DONE   ]"),
                    CampaignStatus::Failed => ("[31m", "[ERROR  ]"),
                };
                println!("  {}{}[0m #{} [{}] {} ({})", color, pill, c.id, c.repo, c.prompt, c.age);
            }
        }
        Some(Commands::Current) => {
            if let Some(curr) = load_current(&config) {
                println!("[1;32mCurrently Active Autonomous Campaign:[0m");
                println!("  Task ID: {}", curr.task_id);
                println!("  Repo:    {}", curr.repo);
                println!("  Branch:  {}", curr.branch);
                println!("  Model:   {}", curr.model);
                println!("  Status:  {}", curr.status);
                if let Some(iter) = curr.iteration {
                    let max_i = curr.max_iterations.unwrap_or(20);
                    println!("  Loop:    Iteration #{}/{}", iter, max_i);
                }
                println!("
Launch 'autopilot' to enter the live control deck.");
            } else {
                println!("No autonomous campaign currently executing.");
            }
        }
        Some(Commands::History { count }) => {
            let hist_file = config.history_file();
            if hist_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&hist_file) {
                    println!("[1m=== Recent Campaign History ===[0m");
                    let lines: Vec<&str> = content.lines().collect();
                    for line in lines.into_iter().rev().take(count) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                            let task = v.get("task").and_then(|t| t.as_str()).unwrap_or("?");
                            let repo = v.get("repo").and_then(|r| r.as_str()).unwrap_or("?");
                            let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("?");
                            let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("?");
                            let color = if status == "completed" { "[32m" } else { "[31m" };
                            println!("  Campaign #{} [{}] -> {}{}[0m ({})", task, repo, color, status, model);
                        }
                    }
                }
            } else {
                println!("No history recorded yet.");
            }
        }
        Some(Commands::Logs { follow, lines, last }) => {
            let log_path = if last {
                let logs_dir = config.logs_dir();
                let mut entries: Vec<_> = std::fs::read_dir(&logs_dir)
                    .unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.file_name().map(|f| f.to_string_lossy().starts_with("codex_")).unwrap_or(false))
                    .collect();
                entries.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH));
                entries.pop()
            } else {
                Some(config.logs_dir().join("worker.log"))
            };

            if let Some(target) = log_path {
                if target.exists() {
                    let mut cmd = Command::new("tail");
                    if follow {
                        cmd.arg("-f");
                    } else {
                        cmd.arg(format!("-n{}", lines));
                    }
                    cmd.arg(&target);
                    let _ = cmd.status();
                } else {
                    println!("Log file {:?} not found.", target);
                }
            } else {
                println!("No logs found.");
            }
        }
        Some(Commands::Daemon) => {
            let supervisor = Supervisor::new(config);
            supervisor.run_daemon().await?;
        }
    }

    Ok(())
}
