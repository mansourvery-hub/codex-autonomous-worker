use std::fs;
use std::path::{Path, PathBuf};
use crate::config::AppConfig;

pub struct PromptManager;

impl PromptManager {
    pub fn get_prompts_dir(config: &AppConfig) -> PathBuf {
        let candidate = PathBuf::from("/home/ubuntu/github-projects/codex-autonomous-worker/prompts");
        if candidate.exists() {
            return candidate;
        }
        let srv_candidate = config.base_dir.join("prompts");
        if srv_candidate.exists() {
            return srv_candidate;
        }
        PathBuf::from("prompts")
    }

    pub fn load_prompt(config: &AppConfig, filename: &str, fallback: &str) -> String {
        let dir = Self::get_prompts_dir(config);
        let path = dir.join(filename);
        if let Ok(content) = fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if filename == "system_prompt.md" {
            let legacy = Path::new("/home/ubuntu/codex-worker/task_prompt.md");
            if let Ok(content) = fs::read_to_string(legacy) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        fallback.to_string()
    }

    pub fn system_prompt(config: &AppConfig) -> String {
        Self::load_prompt(
            config,
            "system_prompt.md",
            include_str!("../prompts/task_prompt.md"),
        )
    }

    pub fn heartbeat(config: &AppConfig, iteration: u32, max_iterations: u32) -> String {
        let template = Self::load_prompt(
            config,
            "heartbeat.md",
            "[Supervisor Heartbeat - Iteration #{iteration}/{max_iterations}] Checkpoint recorded. Proceed with your systematic workflow: check PLAN.md for next item, write test first (red), implement fix (green), verify, and mark complete.",
        );
        template
            .replace("{iteration}", &iteration.to_string())
            .replace("{max_iterations}", &max_iterations.to_string())
    }

    pub fn recovery(config: &AppConfig) -> String {
        Self::load_prompt(
            config,
            "recovery.md",
            "[Supervisor Recovery] An API error occurred on the previous request. Please proceed with your plan using direct file inspection and editing.",
        )
    }

    pub fn rate_limit_cooldown(config: &AppConfig) -> String {
        Self::load_prompt(
            config,
            "rate_limit.md",
            "[Supervisor Recovery] Cooldown complete. Please retry your last action and continue your plan.",
        )
    }

    pub fn default_task(config: &AppConfig) -> String {
        Self::load_prompt(
            config,
            "default_task.md",
            "Audit domain logic, write reproduction tests first, fix edge cases, and execute PLAN.md iteratively",
        )
    }
}
