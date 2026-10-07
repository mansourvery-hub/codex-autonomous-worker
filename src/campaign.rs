use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CampaignStatus {
    Pending,
    Claimed,
    Running,
    Done,
    Failed,
}

impl CampaignStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CampaignStatus::Pending => "pending",
            CampaignStatus::Claimed => "claimed",
            CampaignStatus::Running => "running",
            CampaignStatus::Done => "done",
            CampaignStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentExecution {
    pub task_id: String,
    pub repo: String,
    pub branch: String,
    pub started_at: String,
    pub model: String,
    pub status: String,
    pub iteration: Option<u32>,
    pub max_iterations: Option<u32>,
    pub last_checkpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub repo: String,
    pub prompt: String,
    pub status: CampaignStatus,
    pub mode: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub branch: String,
    pub model: String,
    pub agent: String,
    pub age: String,
    pub file_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawTaskFile {
    pub id: Option<String>,
    pub repo: Option<String>,
    pub prompt: Option<String>,
    pub status: Option<String>,
    pub mode: Option<String>,
    pub iterations: Option<u32>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub priority: Option<String>,
    pub updated_at: Option<String>,
}

pub fn format_duration_ago(mtime: SystemTime) -> String {
    let now = SystemTime::now();
    let elapsed = now.duration_since(mtime).unwrap_or_default().as_secs();
    if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else {
        format!("{}h {:02}m ago", elapsed / 3600, (elapsed % 3600) / 60)
    }
}

pub fn load_current(config: &AppConfig) -> Option<CurrentExecution> {
    let current_path = config.current_file();
    if current_path.exists() {
        if let Ok(content) = fs::read_to_string(&current_path) {
            if let Ok(curr) = serde_json::from_str(&content) {
                return Some(curr);
            }
        }
    }
    None
}

pub fn load_all_campaigns(config: &AppConfig) -> Vec<Campaign> {
    let tasks_dir = config.tasks_dir();
    let mut campaigns = Vec::new();
    let current = load_current(config);

    let entries = match fs::read_dir(&tasks_dir) {
        Ok(e) => e,
        Err(_) => return campaigns,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if fname.starts_with('.') {
            continue;
        }

        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let mut status = CampaignStatus::Pending;
        if fname.contains(".done.") || fname.ends_with(".done.yaml") || fname.ends_with(".done.json") {
            status = CampaignStatus::Done;
        } else if fname.contains(".failed.") || fname.ends_with(".failed.yaml") || fname.ends_with(".failed.json") {
            status = CampaignStatus::Failed;
        } else if fname.contains(".claimed.") || fname.ends_with(".claimed.yaml") || fname.ends_with(".claimed.json") {
            status = CampaignStatus::Claimed;
        }

        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let task_id = stem.split('.').next().unwrap_or("").split('-').next().unwrap_or("").to_string();

        let is_active = current.as_ref().map(|c| c.task_id == task_id).unwrap_or(false);
        if is_active {
            status = CampaignStatus::Running;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let raw: RawTaskFile = if path.extension().map(|e| e == "json").unwrap_or(false) {
            serde_json::from_str(&content).unwrap_or(RawTaskFile {
                id: None, repo: None, prompt: None, status: None, mode: None, iterations: None, model: None, agent: None, priority: None, updated_at: None,
            })
        } else {
            serde_yaml::from_str(&content).unwrap_or(RawTaskFile {
                id: None, repo: None, prompt: None, status: None, mode: None, iterations: None, model: None, agent: None, priority: None, updated_at: None,
            })
        };

        let id = raw.id.unwrap_or_else(|| task_id.clone());
        let repo = raw.repo.unwrap_or_else(|| "unknown".to_string());
        let prompt = raw.prompt.unwrap_or_else(|| stem.clone());
        let mode = raw.mode.unwrap_or_else(|| "continuous".to_string());
        let max_iterations = raw.iterations.unwrap_or(20);
        let model = raw.model.unwrap_or_else(|| config.default_model.clone());
        let agent = raw.agent.unwrap_or_else(|| "codex".to_string());
        let branch = format!("agent/task-{}", id);
        let age = format_duration_ago(mtime);
        let iteration = if is_active {
            current.as_ref().and_then(|c| c.iteration).unwrap_or(1)
        } else {
            max_iterations
        };

        campaigns.push(Campaign {
            id,
            repo,
            prompt,
            status,
            mode,
            iteration,
            max_iterations,
            branch,
            model,
            agent,
            age,
            file_path: path,
        });
    }

    // Sort order: Running (0) -> Claimed (1) -> Pending (2) -> Done (3) -> Failed (4)
    campaigns.sort_by_key(|c| {
        let order = match c.status {
            CampaignStatus::Running => 0,
            CampaignStatus::Claimed => 1,
            CampaignStatus::Pending => 2,
            CampaignStatus::Done => 3,
            CampaignStatus::Failed => 4,
        };
        (order, c.id.clone())
    });

    campaigns
}

pub fn find_session_for_campaign(task_id: &str) -> Option<String> {
    let sessions_dir = dirs::home_dir()?.join(".codex").join("sessions");
    if !sessions_dir.exists() {
        return None;
    }

    let search_tag = format!("task-{}", task_id);
    let mut files = Vec::new();
    find_files_recursive(&sessions_dir, &mut files);

    // Sort newest first
    files.sort_by(|a, b| {
        let ma = fs::metadata(a).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let mb = fs::metadata(b).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    for file in files {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Some(first_line) = content.lines().next() {
                if first_line.contains(&search_tag) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(first_line) {
                        if let Some(session_id) = v.get("payload").and_then(|p| p.get("session_id")).and_then(|s| s.as_str()) {
                            return Some(session_id.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_files_recursive(dir: &Path, acc: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_files_recursive(&path, acc);
            } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                acc.push(path);
            }
        }
    }
}

pub fn find_log_for_campaign(config: &AppConfig, task_id: &str) -> Option<PathBuf> {
    let hist_file = config.history_file();
    if hist_file.exists() {
        if let Ok(content) = fs::read_to_string(&hist_file) {
            for line in content.lines().rev() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    let tid = v.get("task").and_then(|t| t.as_str()).unwrap_or("");
                    if tid == task_id {
                        if let Some(log_path) = v.get("log_path").and_then(|l| l.as_str()) {
                            let p = PathBuf::from(log_path);
                            if p.exists() {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
    }

    let logs_dir = config.logs_dir();
    let mut logs: Vec<PathBuf> = fs::read_dir(&logs_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|f| f.to_string_lossy().contains(task_id))
                .unwrap_or(false)
        })
        .collect();

    logs.sort_by(|a, b| {
        let ma = fs::metadata(a).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let mb = fs::metadata(b).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    logs.into_iter().next()
}

pub fn cancel_running_campaign(config: &AppConfig) -> Result<Option<String>> {
    if let Some(curr) = load_current(config) {
        let _ = std::process::Command::new("tmux").args(["kill-session", "-t", "codex-live"]).output();
        let _ = fs::remove_file(config.current_file());
        let claimed_file = config.tasks_dir().join(format!("{}-campaign.claimed.yaml", curr.task_id));
        if claimed_file.exists() {
            let done_file = config.tasks_dir().join(format!("{}-campaign.stopped.done.yaml", curr.task_id));
            let _ = fs::rename(&claimed_file, done_file);
        }
        return Ok(Some(curr.task_id));
    }
    Ok(None)
}

pub fn queue_campaign(
    config: &AppConfig,
    repo: &str,
    prompt: &str,
    mode: &str,
    iterations: u32,
    model: Option<&str>,
    agent: Option<&str>,
) -> Result<String> {
    let tasks_dir = config.tasks_dir();
    fs::create_dir_all(&tasks_dir)?;

    let mut existing_nums = Vec::new();
    if let Ok(entries) = fs::read_dir(&tasks_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let num_str = fname.split('.').next().unwrap_or("").split('-').next().unwrap_or("");
            if let Ok(n) = num_str.parse::<u32>() {
                existing_nums.push(n);
            }
        }
    }

    let next_id = format!("{:03}", existing_nums.iter().max().unwrap_or(&0) + 1);
    let filename = format!("{}-campaign.yaml", next_id);
    let file_path = tasks_dir.join(filename);

    let raw = RawTaskFile {
        id: Some(next_id.clone()),
        repo: Some(repo.to_string()),
        prompt: Some(prompt.to_string()),
        status: Some("pending".to_string()),
        mode: Some(mode.to_string()),
        iterations: Some(iterations),
        model: Some(model.unwrap_or(&config.default_model).to_string()),
        agent: Some(agent.unwrap_or("codex").to_string()),
        priority: Some("high".to_string()),
        updated_at: Some(Utc::now().to_rfc3339()),
    };

    let yaml_str = serde_yaml::to_string(&raw).context("Failed to serialize campaign to YAML")?;
    fs::write(&file_path, yaml_str).context("Failed to write campaign file")?;

    Ok(next_id)
}
