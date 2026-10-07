use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliproxyConfig {
    #[serde(default = "default_cliproxy_url")]
    pub url: String,
    #[serde(default = "default_catalog_json")]
    pub catalog_json: String,
    #[serde(default = "default_true")]
    pub check_health: bool,
}

fn default_cliproxy_url() -> String {
    "http://127.0.0.1:8317/v1".to_string()
}
fn default_catalog_json() -> String {
    "/home/ubuntu/.codex/model-catalogs/gateway.json".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for CliproxyConfig {
    fn default() -> Self {
        Self {
            url: default_cliproxy_url(),
            catalog_json: default_catalog_json(),
            check_health: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_base_dir")]
    pub base_dir: PathBuf,
    pub repos_dir: Option<PathBuf>,
    pub worktrees_dir: Option<PathBuf>,
    pub tasks_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub logs_dir: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    pub task_timeout_seconds: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_seconds: u64,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_backoff")]
    pub rate_limit_backoff_seconds: u64,
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default)]
    pub cliproxy: CliproxyConfig,
}

fn default_base_dir() -> PathBuf {
    PathBuf::from("/home/ubuntu/srv-codex")
}
fn default_timeout() -> u64 {
    7200
}
fn default_idle_timeout() -> u64 {
    30
}
fn default_poll_interval() -> u64 {
    15
}
fn default_backoff() -> u64 {
    300
}
fn default_model() -> String {
    "antigravity/gemini-3.8-flash-high".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_dir: default_base_dir(),
            repos_dir: None,
            worktrees_dir: None,
            tasks_dir: None,
            state_dir: None,
            logs_dir: None,
            task_timeout_seconds: default_timeout(),
            idle_timeout_seconds: default_idle_timeout(),
            poll_interval_seconds: default_poll_interval(),
            rate_limit_backoff_seconds: default_backoff(),
            default_model: default_model(),
            cliproxy: CliproxyConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let config_path = std::env::var("AUTOPILOT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/home/ubuntu/codex-worker/config.json"));

        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(cfg) = serde_json::from_str(&content) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.tasks_dir.clone().unwrap_or_else(|| self.base_dir.join("tasks"))
    }
    pub fn repos_dir(&self) -> PathBuf {
        self.repos_dir.clone().unwrap_or_else(|| self.base_dir.join("repos"))
    }
    pub fn worktrees_dir(&self) -> PathBuf {
        self.worktrees_dir.clone().unwrap_or_else(|| self.base_dir.join("worktrees"))
    }
    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone().unwrap_or_else(|| self.base_dir.join("state"))
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.logs_dir.clone().unwrap_or_else(|| self.base_dir.join("logs"))
    }
    pub fn current_file(&self) -> PathBuf {
        self.state_dir().join("current.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.state_dir().join("history.jsonl")
    }
    pub fn failures_file(&self) -> PathBuf {
        self.state_dir().join("failures.jsonl")
    }
}
