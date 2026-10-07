use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;
use crate::campaign::{load_all_campaigns, load_current, Campaign, CampaignStatus};
use crate::config::AppConfig;

const TMUX_SESSION_NAME: &str = "codex-live";

pub struct Supervisor {
    config: AppConfig,
}

impl Supervisor {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run_daemon(&self) -> Result<()> {
        println!("Autopilot 24/7 Autonomous Engineering Daemon started.");
        println!("Tasks directory: {:?}", self.config.tasks_dir());

        // 1. Check for active or orphaned campaign from previous run
        if let Some(curr) = load_current(&self.config) {
            let has_session = Command::new("tmux").args(["has-session", "-t", TMUX_SESSION_NAME]).output();
            if has_session.map(|o| o.status.success()).unwrap_or(false) {
                println!("Reconnecting supervisor to active campaign #{}...", curr.task_id);
                let worktree_dir = self.config.worktrees_dir().join(format!("task-{}", curr.task_id));
                let repo_dir = self.resolve_repo_dir(&curr.repo).unwrap_or_else(|_| self.config.base_dir.clone());
                let current_iter = curr.iteration.unwrap_or(1);
                let max_iter = curr.max_iterations.unwrap_or(30);

                let success = self.monitor_session(&worktree_dir, current_iter, max_iter).unwrap_or(false);

                let claimed_path = self.config.tasks_dir().join(format!("{}-campaign.claimed.yaml", curr.task_id));
                if success {
                    let _ = self.mark_status(&claimed_path, "done");
                } else {
                    let _ = self.mark_status(&claimed_path, "failed");
                }
                let _ = fs::remove_file(self.config.current_file());
                self.cleanup_worktree(&repo_dir, &worktree_dir);
            } else {
                println!("Cleaning up stale campaign #{} whose tmux session died.", curr.task_id);
                let _ = fs::remove_file(self.config.current_file());
            }
        }

        let mut consecutive_empty = 0;

        loop {
            // Find next pending task
            let campaigns = load_all_campaigns(&self.config);
            let pending_opt = campaigns.into_iter().find(|c| c.status == CampaignStatus::Pending);

            if let Some(mut campaign) = pending_opt {
                consecutive_empty = 0;
                println!("Claiming campaign #{}: {}", campaign.id, campaign.prompt);

                let claimed_path = self.mark_status(&campaign.file_path, "claimed")?;
                campaign.file_path = claimed_path.clone();

                match self.execute_campaign(&campaign) {
                    Ok(true) => {
                        println!("Campaign #{} finished successfully.", campaign.id);
                        let _ = self.mark_status(&claimed_path, "done");
                    }
                    Ok(false) => {
                        println!("Campaign #{} ended with failure.", campaign.id);
                        let _ = self.mark_status(&claimed_path, "failed");
                    }
                    Err(e) => {
                        eprintln!("Error executing campaign #{}: {:?}", campaign.id, e);
                        let _ = self.mark_status(&claimed_path, "failed");
                    }
                }
            } else {
                consecutive_empty += 1;
                if consecutive_empty % 10 == 0 {
                    println!("Queue empty. Standing by for campaigns in {:?}...", self.config.tasks_dir());
                }
                tokio::time::sleep(Duration::from_secs(self.config.poll_interval_seconds)).await;
            }
        }
    }

    fn mark_status(&self, path: &Path, status: &str) -> Result<PathBuf> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let _file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();

        let clean_stem = stem.replace(".claimed", "").replace(".done", "").replace(".failed", "");
        let new_name = format!("{}.{}.{}", clean_stem, status, ext);
        let new_path = parent.join(new_name);

        if path.exists() && path != new_path {
            fs::rename(path, &new_path)?;
        }
        Ok(new_path)
    }

    fn resolve_repo_dir(&self, repo_name: &str) -> Result<PathBuf> {
        let cand1 = self.config.repos_dir().join(repo_name);
        if cand1.exists() {
            return Ok(cand1);
        }
        let cand2 = PathBuf::from(format!("/home/ubuntu/github-projects/{}", repo_name));
        if cand2.exists() {
            return Ok(cand2);
        }
        let cand3 = PathBuf::from("/home/ubuntu/github-projects/codex-autonomous-worker");
        if cand3.exists() {
            return Ok(cand3);
        }
        bail!("Repository '{}' not found", repo_name);
    }

    fn setup_worktree(&self, repo_path: &Path, branch_name: &str, worktree_path: &Path) -> Result<()> {
        if worktree_path.exists() {
            let _ = Command::new("git")
                .args(["-C", &repo_path.to_string_lossy(), "worktree", "remove", "--force", &worktree_path.to_string_lossy()])
                .output();
            let _ = fs::remove_dir_all(worktree_path);
        }

        let _ = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "fetch", "--all"])
            .output();

        let base_ref_out = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "rev-parse", "--verify", "main"])
            .output();
        let base_ref = if base_ref_out.map(|o| o.status.success()).unwrap_or(false) {
            "main"
        } else {
            "master"
        };

        let add_out = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "worktree", "add", "-b", branch_name, &worktree_path.to_string_lossy(), base_ref])
            .output()?;

        if !add_out.status.success() {
            let add_existing = Command::new("git")
                .args(["-C", &repo_path.to_string_lossy(), "worktree", "add", &worktree_path.to_string_lossy(), branch_name])
                .output()?;
            if !add_existing.status.success() {
                bail!("Failed to create worktree: {}", String::from_utf8_lossy(&add_existing.stderr));
            }
        }

        Ok(())
    }

    fn cleanup_worktree(&self, repo_path: &Path, worktree_path: &Path) {
        let _ = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "worktree", "remove", "--force", &worktree_path.to_string_lossy()])
            .output();
        let _ = fs::remove_dir_all(worktree_path);
    }

    fn send_keys_to_pane(&self, text: &str) {
        // Clear input field without sending SIGINT (C-c kills Codex!)
        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, "Escape"]).output();
        std::thread::sleep(Duration::from_millis(150));
        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, "C-u"]).output();
        std::thread::sleep(Duration::from_millis(150));
        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, text]).output();
        std::thread::sleep(Duration::from_millis(500));
        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, "C-m"]).output();
    }

    fn execute_campaign(&self, campaign: &Campaign) -> Result<bool> {
        let repo_dir = self.resolve_repo_dir(&campaign.repo)?;
        let worktree_dir = self.config.worktrees_dir().join(format!("task-{}", campaign.id));
        let branch_name = format!("agent/task-{}-{}", campaign.id, Utc::now().timestamp());

        // Update current.json
        let current_data = json!({
            "task_id": campaign.id,
            "repo": campaign.repo,
            "branch": branch_name,
            "started_at": Utc::now().to_rfc3339(),
            "model": campaign.model,
            "tmux_session": TMUX_SESSION_NAME,
            "status": "running",
            "iteration": 1,
            "max_iterations": campaign.max_iterations
        });
        let _ = fs::create_dir_all(self.config.state_dir());
        let _ = fs::write(self.config.current_file(), serde_json::to_string_pretty(&current_data)?);

        self.setup_worktree(&repo_dir, &branch_name, &worktree_dir)?;

        let system_prompt_file = Path::new("/home/ubuntu/codex-worker/task_prompt.md");
        let system_prompt = fs::read_to_string(system_prompt_file).unwrap_or_default();
        let full_prompt = format!("{}

Task Description:
{}", system_prompt, campaign.prompt);

        let cliproxy_url = &self.config.cliproxy.url;
        let catalog_json = &self.config.cliproxy.catalog_json;

        // Kill any previous live session
        let _ = Command::new("tmux").args(["kill-session", "-t", TMUX_SESSION_NAME]).output();

        // Start new tmux session
        let res = Command::new("tmux")
            .args(["new-session", "-d", "-s", TMUX_SESSION_NAME, "-x", "140", "-y", "45", "-c", &worktree_dir.to_string_lossy()])
            .output()?;
        if !res.status.success() {
            bail!("Failed to start tmux session: {}", String::from_utf8_lossy(&res.stderr));
        }

        // Send codex command
        let escaped_prompt = full_prompt.replace(char::from(39), "'\''");
        let codex_cmd = format!(
            "codex -C '{}' -c openai_base_url='{}' -c model='{}' -c model_catalog_json='{}' --dangerously-bypass-approvals-and-sandbox '{}'",
            worktree_dir.display(), cliproxy_url, campaign.model, catalog_json, escaped_prompt
        );
        self.send_keys_to_pane(&codex_cmd);

        let success = self.monitor_session(&worktree_dir, 1, campaign.max_iterations)?;

        // Check uncommitted changes
        let status_out = Command::new("git")
            .args(["-C", &worktree_dir.to_string_lossy(), "status", "--porcelain"])
            .output()?;
        let has_changes = !status_out.stdout.is_empty();

        if success && has_changes {
            let _ = Command::new("git")
                .args(["-C", &worktree_dir.to_string_lossy(), "add", "-A"])
                .output();
            let _ = Command::new("git")
                .args(["-C", &worktree_dir.to_string_lossy(), "commit", "-m", &format!("fix(agent): autonomous resolution for campaign {}", campaign.id)])
                .output();
        }

        // Clean current.json & worktree
        let _ = fs::remove_file(self.config.current_file());
        self.cleanup_worktree(&repo_dir, &worktree_dir);

        Ok(success)
    }

    fn monitor_session(
        &self,
        worktree_path: &Path,
        start_iteration: u32,
        max_iterations: u32,
    ) -> Result<bool> {
        let timeout = Duration::from_secs(self.config.task_timeout_seconds);
        let start_time = Instant::now();
        let mut current_iteration = start_iteration;
        let mut consecutive_idle_seconds = 0;
        let mut consecutive_errors = 0;
        let idle_limit = self.config.idle_timeout_seconds.max(15);

        while start_time.elapsed() < timeout {
            // Check if tmux session still exists
            let has_session = Command::new("tmux").args(["has-session", "-t", TMUX_SESSION_NAME]).output()?;
            if !has_session.status.success() {
                break;
            }

            // Capture pane text
            let pane_out = Command::new("tmux").args(["capture-pane", "-pt", TMUX_SESSION_NAME]).output()?;
            let pane_text = String::from_utf8_lossy(&pane_out.stdout);

            let is_working = pane_text.contains("esc to interrupt") || pane_text.contains("Working (") || pane_text.contains("Thinking");

            let has_error = pane_text.contains(r#""type":"error""#)
                || pane_text.contains("status code: 400")
                || pane_text.contains(r#""status":400"#)
                || pane_text.contains("502 Bad Gateway")
                || pane_text.contains(r#""status":502"#)
                || pane_text.contains("429 Too Many Requests")
                || pane_text.contains(r#""status":429"#)
                || pane_text.contains("rate_limit");

            let is_idle_at_prompt = !is_working && (pane_text.contains("Ask Codex to do anything") || pane_text.contains('›'));

            // Handle upstream errors immediately if sitting idle at prompt
            if has_error && is_idle_at_prompt {
                consecutive_errors += 1;
                if consecutive_errors >= 4 {
                    eprintln!("Campaign encountered 4 consecutive unrecoverable upstream errors. Concluding session...");
                    let _ = Command::new("tmux").args(["kill-session", "-t", TMUX_SESSION_NAME]).output();
                    return Ok(false);
                }

                if pane_text.contains("429") || pane_text.contains("rate_limit") || pane_text.contains("502") {
                    let backoff = self.config.rate_limit_backoff_seconds.min(60).max(15);
                    println!("Upstream rate-limit/gateway error (429/502). Cooling down for {}s (attempt {}/4)...", backoff, consecutive_errors);
                    std::thread::sleep(Duration::from_secs(backoff));
                    self.send_keys_to_pane("[Supervisor Recovery] Cooldown complete. Please retry your last action and continue your plan.");
                } else {
                    println!("Upstream API error detected. Injecting recovery directive (attempt {}/4)...", consecutive_errors);
                    std::thread::sleep(Duration::from_secs(5));
                    self.send_keys_to_pane("[Supervisor Recovery] An API error occurred on the previous request. Please proceed with your plan using direct file inspection and editing.");
                }
                consecutive_idle_seconds = 0;
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }

            if is_idle_at_prompt {
                consecutive_errors = 0;
                consecutive_idle_seconds += 2;

                if consecutive_idle_seconds >= idle_limit {
                    if current_iteration < max_iterations {
                        println!("Iteration {} complete. Checkpointing and injecting heartbeat directive #{}/{}...", current_iteration, current_iteration + 1, max_iterations);

                        // 1. Git checkpoint
                        let status_out = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "status", "--porcelain"]).output()?;
                        if !status_out.stdout.is_empty() {
                            let _ = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "add", "-A"]).output();
                            let _ = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "commit", "-m", &format!("checkpoint(agent): iteration {} autonomous progress", current_iteration)]).output();
                        }

                        current_iteration += 1;

                        // Update current.json
                        if let Ok(content) = fs::read_to_string(self.config.current_file()) {
                            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) {
                                v["iteration"] = json!(current_iteration);
                                v["last_checkpoint"] = json!(Utc::now().to_rfc3339());
                                let _ = fs::write(self.config.current_file(), serde_json::to_string_pretty(&v).unwrap_or_default());
                            }
                        }

                        // 2. Inject heartbeat using two-step send-keys
                        let directive = format!(
                            "[Supervisor Heartbeat - Iteration #{}/{}] Checkpoint recorded. Proceed with your systematic workflow: check PLAN.md for next item, write test first (red), implement fix (green), verify, and mark complete.",
                            current_iteration, max_iterations
                        );
                        self.send_keys_to_pane(&directive);

                        consecutive_idle_seconds = 0;
                        std::thread::sleep(Duration::from_secs(3));
                        continue;
                    } else {
                        println!("Autonomous session concluding (iteration {}/{} reached)...", current_iteration, max_iterations);
                        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, "C-c"]).output();
                        std::thread::sleep(Duration::from_millis(500));
                        let _ = Command::new("tmux").args(["send-keys", "-t", TMUX_SESSION_NAME, "C-d"]).output();
                        std::thread::sleep(Duration::from_secs(1));
                        let _ = Command::new("tmux").args(["kill-session", "-t", TMUX_SESSION_NAME]).output();
                        break;
                    }
                }
            } else {
                consecutive_idle_seconds = 0;
            }

            std::thread::sleep(Duration::from_secs(2));
        }

        let _ = Command::new("tmux").args(["kill-session", "-t", TMUX_SESSION_NAME]).output();
        let completed = current_iteration >= max_iterations;
        if completed {
            println!("Campaign reached iteration budget ({}/{}). Concluded.", current_iteration, max_iterations);
        } else {
            println!("Session ended at iteration {}/{}.", current_iteration, max_iterations);
        }
        Ok(completed)
    }
}
