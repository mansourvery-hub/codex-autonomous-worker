use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;
use tokio::sync::{Mutex, Semaphore};
use crate::campaign::{load_all_campaigns, Campaign, CampaignStatus};
use crate::config::AppConfig;
use crate::prompts::PromptManager;

static GIT_WORKTREE_LOCK: StdMutex<()> = StdMutex::new(());

pub struct Supervisor {
    config: AppConfig,
    active_tasks: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
}

impl Supervisor {
    pub fn new(config: AppConfig) -> Self {
        let max_concurrent = config.max_concurrent_tasks.max(1);
        Self {
            config,
            active_tasks: Arc::new(Mutex::new(HashSet::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    pub async fn run_daemon(self: Arc<Self>) -> Result<()> {
        println!(
            "Autopilot 24/7 Autonomous Engineering Daemon started (parallel capacity: {}).",
            self.config.max_concurrent_tasks
        );
        println!("Tasks directory: {:?}", self.config.tasks_dir());

        // 1. Recover and reconnect any active parallel sessions
        self.recover_active_sessions().await;

        let mut consecutive_empty = 0;

        loop {
            let campaigns = load_all_campaigns(&self.config);
            let pending_list: Vec<_> = campaigns
                .into_iter()
                .filter(|c| c.status == CampaignStatus::Pending)
                .collect();

            if pending_list.is_empty() {
                consecutive_empty += 1;
                if consecutive_empty % 15 == 0 {
                    let active_cnt = self.active_tasks.lock().await.len();
                    println!(
                        "Standing by for new campaigns ({} currently active in parallel)...",
                        active_cnt
                    );
                }
                tokio::time::sleep(Duration::from_secs(self.config.poll_interval_seconds.min(2))).await;
                continue;
            }

            consecutive_empty = 0;

            for campaign in pending_list {
                let mut active_lock = self.active_tasks.lock().await;
                if active_lock.contains(&campaign.id) {
                    continue;
                }

                // Acquire parallel slot
                match self.semaphore.clone().try_acquire_owned() {
                    Ok(permit) => {
                        println!(
                            "Claiming campaign #{} for {} (engine: {}) [parallel slot acquired]",
                            campaign.id, campaign.repo, campaign.agent
                        );

                        let claimed_path = match self.mark_status(&campaign.file_path, "claimed") {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("Failed to claim campaign #{}: {:?}", campaign.id, e);
                                continue;
                            }
                        };

                        active_lock.insert(campaign.id.clone());
                        drop(active_lock);

                        let mut c_clone = campaign.clone();
                        c_clone.file_path = claimed_path;
                        let self_arc = Arc::clone(&self);

                        tokio::spawn(async move {
                            let _permit = permit;
                            let cid = c_clone.id.clone();
                            let claimed_file = c_clone.file_path.clone();

                            let self_exec = Arc::clone(&self_arc);
                            let outcome = tokio::task::spawn_blocking(move || {
                                self_exec.execute_campaign(&c_clone)
                            })
                            .await;

                            match outcome {
                                Ok(Ok(true)) => {
                                    println!("Campaign #{} finished successfully.", cid);
                                    let _ = self_arc.mark_status(&claimed_file, "done");
                                }
                                Ok(Ok(false)) => {
                                    println!("Campaign #{} concluded with incomplete/failure state.", cid);
                                    let _ = self_arc.mark_status(&claimed_file, "failed");
                                }
                                Ok(Err(e)) => {
                                    eprintln!("Error executing campaign #{}: {:?}", cid, e);
                                    let _ = self_arc.mark_status(&claimed_file, "failed");
                                }
                                Err(join_err) => {
                                    eprintln!("Campaign #{} thread panicked: {:?}", cid, join_err);
                                    let _ = self_arc.mark_status(&claimed_file, "failed");
                                }
                            }

                            // Clean up task state file
                            let state_file = self_arc.config.state_dir().join(format!("task-{}.json", cid));
                            let _ = fs::remove_file(state_file);

                            self_arc.active_tasks.lock().await.remove(&cid);
                        });
                    }
                    Err(_) => {
                        // All parallel slots occupied, wait for an active task to finish
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn recover_active_sessions(&self) {
        let campaigns = load_all_campaigns(&self.config);
        let claimed_list: Vec<_> = campaigns
            .into_iter()
            .filter(|c| c.status == CampaignStatus::Claimed || c.status == CampaignStatus::Running)
            .collect();

        for c in claimed_list {
            let session_name = format!("autopilot-{}", c.id);
            let has_session = Command::new("tmux").args(["has-session", "-t", &session_name]).output();
            let is_alive = has_session.map(|o| o.status.success()).unwrap_or(false);

            let has_legacy = Command::new("tmux").args(["has-session", "-t", "codex-live"]).output();
            let is_legacy_alive = has_legacy.map(|o| o.status.success()).unwrap_or(false);

            if is_alive || (c.id == "011" && is_legacy_alive) {
                let live_session = if is_alive { session_name } else { "codex-live".to_string() };
                println!("Reconnecting supervisor to live session '{}' for campaign #{}...", live_session, c.id);
                self.active_tasks.lock().await.insert(c.id.clone());

                let self_clone = Arc::new(self.clone());
                let worktree_dir = self.config.worktrees_dir().join(format!("task-{}", c.id));
                let claimed_file = c.file_path.clone();
                let cid = c.id.clone();
                let max_iter = c.max_iterations;

                let self_monitor = Arc::clone(&self_clone);
                let sess_for_task = live_session.clone();
                tokio::spawn(async move {
                    let cid_blk = cid.clone();
                    let outcome = tokio::task::spawn_blocking(move || {
                        self_monitor.monitor_session(&worktree_dir, &sess_for_task, &cid_blk, 1, max_iter)
                    })
                    .await;

                    if let Ok(Ok(true)) = outcome {
                        let _ = self_clone.mark_status(&claimed_file, "done");
                    } else {
                        let _ = self_clone.mark_status(&claimed_file, "failed");
                    }
                    let state_file = self_clone.config.state_dir().join(format!("task-{}.json", cid));
                    let _ = fs::remove_file(state_file);
                    self_clone.active_tasks.lock().await.remove(&cid);
                });
            } else {
                println!("Cleaning up orphaned claimed campaign #{}...", c.id);
                let _ = self.mark_status(&c.file_path, "failed");
            }
        }
    }

    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            active_tasks: Arc::clone(&self.active_tasks),
            semaphore: Arc::clone(&self.semaphore),
        }
    }

    fn mark_status(&self, path: &Path, status: &str) -> Result<PathBuf> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();

        let clean_stem = stem
            .replace(".claimed", "")
            .replace(".done", "")
            .replace(".failed", "")
            .replace(".stopped", "");

        let new_name = match status {
            "claimed" => format!("{}.claimed.{}", clean_stem, ext),
            "done" => format!("{}.done.{}", clean_stem, ext),
            "failed" => format!("{}.failed.{}", clean_stem, ext),
            _ => format!("{}.{}", clean_stem, ext),
        };

        let new_path = parent.join(new_name);
        if path.exists() && path != new_path {
            fs::rename(path, &new_path)?;
        }
        Ok(new_path)
    }

    fn resolve_repo_dir(&self, repo_name: &str) -> Result<PathBuf> {
        let cand1 = PathBuf::from(format!("/home/ubuntu/github-projects/{}", repo_name));
        if cand1.exists() {
            return Ok(cand1);
        }
        let cand2 = self.config.repos_dir().join(repo_name);
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
        let _guard = GIT_WORKTREE_LOCK.lock().unwrap();

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
        let _guard = GIT_WORKTREE_LOCK.lock().unwrap();
        let _ = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "worktree", "remove", "--force", &worktree_path.to_string_lossy()])
            .output();
        let _ = fs::remove_dir_all(worktree_path);
    }

    fn send_keys_to_session(&self, session_name: &str, text: &str) {
        // Clear input field cleanly without sending raw Escape which unfocuses OpenCode
        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "C-a"]).output();
        std::thread::sleep(Duration::from_millis(50));
        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "C-k"]).output();
        std::thread::sleep(Duration::from_millis(50));
        // Send literal text with -l flag so tmux does not interpret brackets [ as copy mode
        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "-l", text]).output();
        std::thread::sleep(Duration::from_millis(200));
        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "C-m"]).output();
    }

    fn execute_campaign(&self, campaign: &Campaign) -> Result<bool> {
        let repo_dir = self.resolve_repo_dir(&campaign.repo)?;
        let worktree_dir = self.config.worktrees_dir().join(format!("task-{}", campaign.id));
        let branch_name = format!("agent/task-{}-{}", campaign.id, Utc::now().timestamp());
        let session_name = format!("autopilot-{}", campaign.id);

        let system_prompt = PromptManager::system_prompt(&self.config);
        let full_prompt = format!("{}

Task Description:
{}", system_prompt, campaign.prompt);

        self.setup_worktree(&repo_dir, &branch_name, &worktree_dir)?;

        // Write full prompt directly to worktree_dir/prompt.txt
        let prompt_file = worktree_dir.join("prompt.txt");
        fs::write(&prompt_file, &full_prompt)?;

        // If OpenCode, configure model appropriately
        let opencode_model = if campaign.model.is_empty() {
            "cliproxy/antigravity/gemini-3.8-flash-high".to_string()
        } else if campaign.model.starts_with("cliproxy/")
            || campaign.model.starts_with("opencode/")
            || campaign.model.starts_with("bifrost/")
        {
            campaign.model.clone()
        } else {
            format!("cliproxy/{}", campaign.model)
        };

        if campaign.agent == "opencode" {
            let local_cfg = json!({
                "model": opencode_model
            });
            let _ = fs::write(
                worktree_dir.join("opencode.json"),
                serde_json::to_string_pretty(&local_cfg).unwrap_or_default(),
            );
        }

        let cliproxy_url = &self.config.cliproxy.url;
        let catalog_json = &self.config.cliproxy.catalog_json;

        // Generate robust bash launcher script
        let launcher_content = if campaign.agent == "opencode" {
            format!(
                r#"#!/usr/bin/env bash
export PATH="/home/ubuntu/.opencode/bin:/home/ubuntu/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
PROMPT_CONTENT=$(cat "{}")
opencode --auto --prompt "$PROMPT_CONTENT"
EXIT_CODE=$?
echo "OpenCode exited with code $EXIT_CODE"
if [ $EXIT_CODE -ne 0 ]; then
    sleep 30
fi
"#,
                prompt_file.display()
            )
        } else {
            format!(
                r#"#!/usr/bin/env bash
export PATH="/home/ubuntu/.opencode/bin:/home/ubuntu/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
PROMPT_CONTENT=$(cat "{}")
codex -C "{}" -c openai_base_url="{}" -c model="{}" -c model_catalog_json="{}" --dangerously-bypass-approvals-and-sandbox "$PROMPT_CONTENT"
EXIT_CODE=$?
echo "Codex exited with code $EXIT_CODE"
if [ $EXIT_CODE -ne 0 ]; then
    sleep 30
fi
"#,
                prompt_file.display(),
                worktree_dir.display(),
                cliproxy_url,
                campaign.model,
                catalog_json
            )
        };

        let run_agent_path = worktree_dir.join("run_agent.sh");
        fs::write(&run_agent_path, launcher_content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&run_agent_path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&run_agent_path, perms);
            }
        }

        // Write initial task state file
        let task_state = json!({
            "task_id": campaign.id,
            "repo": campaign.repo,
            "branch": branch_name,
            "agent": campaign.agent,
            "model": if campaign.agent == "opencode" { &opencode_model } else { &campaign.model },
            "iteration": 1,
            "max_iterations": campaign.max_iterations,
            "status": "running",
            "started_at": Utc::now().to_rfc3339(),
        });
        let _ = fs::create_dir_all(self.config.state_dir());
        let _ = fs::write(
            self.config.state_dir().join(format!("task-{}.json", campaign.id)),
            serde_json::to_string_pretty(&task_state).unwrap_or_default(),
        );

        // Kill any previous session with this task id
        let _ = Command::new("tmux").args(["kill-session", "-t", &session_name]).output();

        // Start isolated parallel tmux session
        let res = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "140",
                "-y",
                "45",
                "-c",
                &worktree_dir.to_string_lossy(),
                &run_agent_path.to_string_lossy(),
            ])
            .output()?;
        if !res.status.success() {
            bail!("Failed to start tmux session: {}", String::from_utf8_lossy(&res.stderr));
        }

        let success = self.monitor_session(&worktree_dir, &session_name, &campaign.id, 1, campaign.max_iterations)?;

        let status_out = Command::new("git")
            .args(["-C", &worktree_dir.to_string_lossy(), "status", "--porcelain"])
            .output()?;
        let has_changes = !status_out.stdout.is_empty();

        if success && has_changes {
            let _ = Command::new("git")
                .args(["-C", &worktree_dir.to_string_lossy(), "add", "."])
                .output();
            let _ = Command::new("git")
                .args(["-C", &worktree_dir.to_string_lossy(), "commit", "-m", "chore: final checkpoint for campaign"])
                .output();
        }

        self.cleanup_worktree(&repo_dir, &worktree_dir);

        Ok(success)
    }

    fn monitor_session(
        &self,
        worktree_path: &Path,
        session_name: &str,
        task_id: &str,
        start_iteration: u32,
        max_iterations: u32,
    ) -> Result<bool> {
        let mut current_iteration = start_iteration;
        let mut consecutive_idle_seconds = 0;
        let mut consecutive_errors = 0;
        let start_time = Instant::now();
        let timeout = Duration::from_secs(self.config.task_timeout_seconds);
        let idle_limit = self.config.idle_timeout_seconds.max(15);
        std::thread::sleep(Duration::from_millis(1000));

        while start_time.elapsed() < timeout {
            let has_session = Command::new("tmux").args(["has-session", "-t", session_name]).output()?;
            if !has_session.status.success() {
                println!("Session '{}' ended after {:?}.", session_name, start_time.elapsed());
                break;
            }

            let pane_out = Command::new("tmux").args(["capture-pane", "-pt", session_name]).output()?;
            let pane_text = String::from_utf8_lossy(&pane_out.stdout);

            let is_working = pane_text.contains("esc to interrupt")
                || pane_text.contains("esc interrupt")
                || pane_text.contains("Working (");

            let has_error = pane_text.contains(r#""type":"error""#)
                || pane_text.contains("status code: 400")
                || pane_text.contains("502 Bad Gateway")
                || pane_text.contains("500 Internal")
                || pane_text.contains("429 Too Many Requests")
                || pane_text.contains(r#""code":429"#)
                || pane_text.contains(r#""status":429"#)
                || pane_text.contains("rate_limit")
                || pane_text.contains("ConnectionRefused");

            let is_idle_at_prompt = !is_working
                && (pane_text.contains("Ask Codex to do anything")
                    || pane_text.contains("Ask anything")
                    || pane_text.contains("ctrl+p commands")
                    || pane_text.contains("shift+tab")
                    || pane_text.contains('›'));

            if has_error && is_idle_at_prompt {
                consecutive_errors += 1;
                if consecutive_errors >= 4 {
                    eprintln!("Session '{}' encountered 4 consecutive unrecoverable errors. Concluding...", session_name);
                    let _ = Command::new("tmux").args(["kill-session", "-t", session_name]).output();
                    return Ok(false);
                }

                if pane_text.contains("429") || pane_text.contains("rate_limit") || pane_text.contains("502") {
                    let backoff = self.config.rate_limit_backoff_seconds.min(60).max(15);
                    println!("Session '{}': rate-limit/gateway error (429/502). Cooling down for {}s...", session_name, backoff);
                    std::thread::sleep(Duration::from_secs(backoff));
                    let cd_msg = PromptManager::rate_limit_cooldown(&self.config);
                    self.send_keys_to_session(session_name, &cd_msg);
                } else {
                    println!("Session '{}': upstream API error detected. Injecting recovery directive...", session_name);
                    std::thread::sleep(Duration::from_secs(5));
                    let rec_msg = PromptManager::recovery(&self.config);
                    self.send_keys_to_session(session_name, &rec_msg);
                }
                consecutive_idle_seconds = 0;
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }

            if is_idle_at_prompt {
                consecutive_idle_seconds += 2;

                if consecutive_idle_seconds >= idle_limit {
                    if current_iteration < max_iterations {
                        println!(
                            "Session '{}': Iteration {} complete. Checkpointing and injecting heartbeat directive #{}/{}...",
                            session_name, current_iteration, current_iteration + 1, max_iterations
                        );

                        // 1. Git checkpoint
                        let status_out = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "status", "--porcelain"]).output()?;
                        if !status_out.stdout.is_empty() {
                            let _ = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "add", "."]).output();
                            let msg = format!("chore: autopilot continuous loop checkpoint - iteration #{}", current_iteration);
                            let _ = Command::new("git").args(["-C", &worktree_path.to_string_lossy(), "commit", "-m", &msg]).output();
                        }

                        current_iteration += 1;

                        // Update task-{id}.json state
                        let state_file = self.config.state_dir().join(format!("task-{}.json", task_id));
                        if let Ok(content) = fs::read_to_string(&state_file) {
                            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) {
                                v["iteration"] = json!(current_iteration);
                                v["last_checkpoint"] = json!(Utc::now().to_rfc3339());
                                let _ = fs::write(&state_file, serde_json::to_string_pretty(&v).unwrap_or_default());
                            }
                        }

                        // 2. Inject heartbeat
                        let directive = PromptManager::heartbeat(&self.config, current_iteration, max_iterations);
                        self.send_keys_to_session(session_name, &directive);

                        consecutive_idle_seconds = 0;
                        std::thread::sleep(Duration::from_secs(3));
                        continue;
                    } else {
                        println!("Session '{}' reached max iterations ({}/{}). Concluding...", session_name, current_iteration, max_iterations);
                        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "C-c"]).output();
                        std::thread::sleep(Duration::from_millis(500));
                        let _ = Command::new("tmux").args(["send-keys", "-t", session_name, "C-d"]).output();
                        std::thread::sleep(Duration::from_secs(1));
                        let _ = Command::new("tmux").args(["kill-session", "-t", session_name]).output();
                        break;
                    }
                }
            } else {
                consecutive_idle_seconds = 0;
                consecutive_errors = 0;
            }

            std::thread::sleep(Duration::from_secs(2));
        }

        let _ = Command::new("tmux").args(["kill-session", "-t", session_name]).output();
        let completed = current_iteration >= max_iterations;
        Ok(completed)
    }
}
