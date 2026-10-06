#!/usr/bin/env python3
import os
import sys
import time
import json
import yaml
import signal
import shutil
import logging
import datetime
import subprocess
from pathlib import Path

# Load config
CONFIG_FILE = Path(os.environ.get("CODEX_WORKER_CONFIG", "/home/ubuntu/codex-worker/config.json"))
if CONFIG_FILE.exists():
    with open(CONFIG_FILE, "r") as f:
        CONFIG = json.load(f)
else:
    CONFIG = {
        "base_dir": "/home/ubuntu/srv-codex",
        "repos_dir": "/home/ubuntu/srv-codex/repos",
        "worktrees_dir": "/home/ubuntu/srv-codex/worktrees",
        "tasks_dir": "/home/ubuntu/srv-codex/tasks",
        "state_dir": "/home/ubuntu/srv-codex/state",
        "logs_dir": "/home/ubuntu/srv-codex/logs",
        "task_timeout_seconds": 3600,
        "idle_timeout_seconds": 60,
        "poll_interval_seconds": 15,
        "rate_limit_backoff_seconds": 300,
        "max_consecutive_failures": 5,
        "default_model": "gemini-3.5-flash-lite",
        "cliproxy": {
            "url": "http://127.0.0.1:8317/v1",
            "check_health": True,
            "catalog_json": "/home/ubuntu/.codex/model-catalogs/gateway.json"
        }
    }

BASE_DIR = Path(CONFIG.get("base_dir", "/home/ubuntu/srv-codex"))
TASKS_DIR = Path(CONFIG.get("tasks_dir", str(BASE_DIR / "tasks")))
WORKTREES_DIR = Path(CONFIG.get("worktrees_dir", str(BASE_DIR / "worktrees")))
REPOS_DIR = Path(CONFIG.get("repos_dir", str(BASE_DIR / "repos")))
STATE_DIR = Path(CONFIG.get("state_dir", str(BASE_DIR / "state")))
LOGS_DIR = Path(CONFIG.get("logs_dir", str(BASE_DIR / "logs")))

for d in [TASKS_DIR, WORKTREES_DIR, REPOS_DIR, STATE_DIR, LOGS_DIR]:
    d.mkdir(parents=True, exist_ok=True)

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    handlers=[
        logging.StreamHandler(sys.stdout),
        logging.FileHandler(LOGS_DIR / "worker.log")
    ]
)
logger = logging.getLogger("codex-worker")

HISTORY_FILE = STATE_DIR / "history.jsonl"
FAILURES_FILE = STATE_DIR / "failures.jsonl"
CURRENT_FILE = STATE_DIR / "current.json"
TMUX_SESSION_NAME = "codex-live"

RUNNING = True

def handle_sigterm(signum, frame):
    global RUNNING
    logger.info("Termination signal received. Gracefully exiting after current iteration...")
    RUNNING = False

signal.signal(signal.SIGINT, handle_sigterm)
signal.signal(signal.SIGTERM, handle_sigterm)

def record_journal(entry, failure=False):
    target = FAILURES_FILE if failure else HISTORY_FILE
    with open(target, "a") as f:
        f.write(json.dumps(entry) + chr(10))

def get_failed_approaches_for_repo(repo_name):
    notes = []
    if FAILURES_FILE.exists():
        with open(FAILURES_FILE, "r") as f:
            for line in f:
                try:
                    item = json.loads(line)
                    if item.get("repo") == repo_name and item.get("notes"):
                        notes.extend(item.get("notes"))
                except Exception:
                    pass
    return notes[-5:]

def find_next_task():
    task_files = sorted(list(TASKS_DIR.glob("*.yaml")) + list(TASKS_DIR.glob("*.yml")) + list(TASKS_DIR.glob("*.json")))
    for path in task_files:
        if path.name.startswith(".") or ".claimed" in path.name or ".done" in path.name:
            continue
        try:
            with open(path, "r") as f:
                data = yaml.safe_load(f) if path.suffix in [".yaml", ".yml"] else json.load(f)
            if data and data.get("status", "pending") == "pending":
                return path, data
        except Exception as e:
            logger.error(f"Failed to read task file {path}: {e}")
    return None, None

def mark_task_status(path, data, status):
    data["status"] = status
    data["updated_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    target_path = path.with_name(f"{path.stem}.{status}{path.suffix}")
    try:
        with open(target_path, "w") as f:
            if target_path.suffix in [".yaml", ".yml"]:
                yaml.dump(data, f, default_flow_style=False)
            else:
                json.dump(data, f, indent=2)
        if path.exists() and path != target_path:
            path.unlink()
        return target_path
    except Exception as e:
        logger.error(f"Error marking task status: {e}")
        return path

def setup_worktree(repo_path, branch_name, worktree_path):
    if worktree_path.exists():
        subprocess.run(["git", "-C", str(repo_path), "worktree", "remove", "--force", str(worktree_path)], capture_output=True)
        if worktree_path.exists():
            shutil.rmtree(worktree_path, ignore_errors=True)

    subprocess.run(["git", "-C", str(repo_path), "fetch", "--all"], capture_output=True)
    proc = subprocess.run(["git", "-C", str(repo_path), "rev-parse", "--verify", "main"], capture_output=True)
    base_ref = "main" if proc.returncode == 0 else "master"

    cmd = ["git", "-C", str(repo_path), "worktree", "add", "-b", branch_name, str(worktree_path), base_ref]
    logger.info(f"Creating worktree: {' '.join(cmd)}")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        cmd_existing = ["git", "-C", str(repo_path), "worktree", "add", str(worktree_path), branch_name]
        res = subprocess.run(cmd_existing, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"Failed to create worktree: {res.stderr}")
    return True

def cleanup_worktree(repo_path, worktree_path):
    try:
        subprocess.run(["git", "-C", str(repo_path), "worktree", "remove", "--force", str(worktree_path)], capture_output=True)
        if worktree_path.exists():
            shutil.rmtree(worktree_path, ignore_errors=True)
    except Exception as e:
        logger.warning(f"Error cleaning up worktree {worktree_path}: {e}")

def run_codex_job(worktree_path, full_prompt, model, timeout_secs):
    task_log_file = LOGS_DIR / f"codex_live_{int(time.time())}.log"
    cliproxy_info = CONFIG.get("cliproxy", {})
    cliproxy_url = cliproxy_info.get("url", "http://127.0.0.1:8317/v1")
    catalog_json = cliproxy_info.get("catalog_json", "/home/ubuntu/.codex/model-catalogs/gateway.json")
    idle_limit = CONFIG.get("idle_timeout_seconds", 60)

    # Write prompt to file to ensure clean loading
    prompt_file = worktree_path / ".agent_prompt.txt"
    prompt_file.write_text(full_prompt)

    # Launch real interactive codex inside tmux with initial prompt
    cmd = [
        "codex",
        "-C", str(worktree_path),
        "-c", f'openai_base_url="{cliproxy_url}"',
        "-c", f'model="{model}"',
        "-c", f'model_catalog_json="{catalog_json}"',
        "--dangerously-bypass-approvals-and-sandbox",
        full_prompt
    ]

    logger.info(f"Launching real interactive Codex TUI in tmux session '{TMUX_SESSION_NAME}' with model {model} (timeout={timeout_secs}s)")
    start_time = time.time()

    # Clean previous tmux session
    subprocess.run(["tmux", "kill-session", "-t", TMUX_SESSION_NAME], capture_output=True)

    # Start interactive Codex inside tmux window
    res = subprocess.run([
        "tmux", "new-session", "-d", "-s", TMUX_SESSION_NAME,
        "-x", "140", "-y", "45",
        "-c", str(worktree_path)
    ], capture_output=True, text=True)

    if res.returncode != 0:
        logger.error(f"Failed to start tmux session: {res.stderr}")
        return False, f"tmux_error: {res.stderr}", task_log_file

    # Send codex command to pane
    escaped_prompt = full_prompt.replace("'", "'\''")
    codex_cmd = f"codex -C '{worktree_path}' -c openai_base_url='{cliproxy_url}' -c model='{model}' -c model_catalog_json='{catalog_json}' --dangerously-bypass-approvals-and-sandbox '{escaped_prompt}'"
    subprocess.run(["tmux", "send-keys", "-t", TMUX_SESSION_NAME, codex_cmd, "C-m"])

    # Monitor session
    consecutive_idle_seconds = 0
    while time.time() - start_time < timeout_secs:
        # Check if tmux session still exists
        has_session = subprocess.run(["tmux", "has-session", "-t", TMUX_SESSION_NAME], capture_output=True)
        if has_session.returncode != 0:
            # Session closed by user (e.g. via Ctrl-D or /exit)
            logger.info("Session closed naturally by user or process exit.")
            break

        # Capture pane text
        pane_res = subprocess.run(["tmux", "capture-pane", "-pt", TMUX_SESSION_NAME], capture_output=True, text=True)
        pane_text = pane_res.stdout if pane_res.returncode == 0 else ""

        # Check if client is currently attached (human actively viewing/interacting)
        clients_res = subprocess.run(["tmux", "list-clients", "-t", TMUX_SESSION_NAME], capture_output=True, text=True)
        has_client = bool(clients_res.stdout.strip())

        # Check if agent is currently working
        is_working = ("esc to interrupt" in pane_text or "Working" in pane_text or "◦" in pane_text)

        if has_client:
            # Human is actively attached in TUI — never auto-close while human is interacting!
            consecutive_idle_seconds = 0
        elif not is_working and ("Ask Codex to do anything" in pane_text or "›" in pane_text):
            # Agent has completed its turn and is idle at prompt, with no human attached
            consecutive_idle_seconds += 2
            if consecutive_idle_seconds >= idle_limit:
                logger.info(f"Agent finished work and remained idle for {idle_limit}s with no human attached. Concluding session...")
                # Gracefully exit Codex: Ctrl-C then Ctrl-D
                subprocess.run(["tmux", "send-keys", "-t", TMUX_SESSION_NAME, "C-c"])
                time.sleep(0.5)
                subprocess.run(["tmux", "send-keys", "-t", TMUX_SESSION_NAME, "C-d"])
                time.sleep(1.5)
                subprocess.run(["tmux", "kill-session", "-t", TMUX_SESSION_NAME], capture_output=True)
                break
        else:
            consecutive_idle_seconds = 0

        time.sleep(2)

    elapsed = time.time() - start_time

    # Save final pane capture to log
    pane_res = subprocess.run(["tmux", "capture-pane", "-pt", TMUX_SESSION_NAME], capture_output=True, text=True)
    if pane_res.returncode == 0:
        task_log_file.write_text(pane_res.stdout)
    prompt_file.unlink(missing_ok=True)

    # Clean tmux session if still open
    subprocess.run(["tmux", "kill-session", "-t", TMUX_SESSION_NAME], capture_output=True)

    logger.info(f"Codex interactive session concluded in {elapsed:.1f}s")
    return True, "completed", task_log_file

def execute_task(task_file, task_data):
    task_id = str(task_data.get("id", task_file.stem))
    repo_name = task_data.get("repo")
    prompt = task_data.get("prompt", "")
    model = task_data.get("model", CONFIG.get("default_model", "gemini-3.5-flash-lite"))
    branch_name = f"agent/task-{task_id}-{int(time.time())}"
    worktree_path = WORKTREES_DIR / f"task-{task_id}"

    repo_path = REPOS_DIR / repo_name
    if not repo_path.exists():
        cand = Path(f"/home/ubuntu/github-projects/{repo_name}")
        if cand.exists():
            repo_path = cand
    if not repo_path.exists():
        logger.error(f"Repo {repo_name} not found")
        return False, "repo_not_found"

    system_prompt_file = Path("/home/ubuntu/codex-worker/task_prompt.md")
    system_prompt = system_prompt_file.read_text() if system_prompt_file.exists() else ""
    failed_notes = get_failed_approaches_for_repo(repo_name)
    history_context = ""
    if failed_notes:
        history_context = "\n\nFailed approaches from previous runs to avoid:\n" + "\n".join(f"- {n}" for n in failed_notes)

    full_prompt = f"{system_prompt}\n\nTask Description:\n{prompt}{history_context}"

    CURRENT_FILE.write_text(json.dumps({
        "task_id": task_id,
        "repo": repo_name,
        "branch": branch_name,
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "model": model,
        "tmux_session": TMUX_SESSION_NAME,
        "status": "running"
    }, indent=2))

    try:
        setup_worktree(repo_path, branch_name, worktree_path)
    except Exception as e:
        logger.error(f"Worktree setup failed: {e}")
        return False, f"worktree_setup_failed: {e}"

    timeout = task_data.get("timeout_seconds", CONFIG.get("task_timeout_seconds", 3600))
    success, reason, log_path = run_codex_job(worktree_path, full_prompt, model, timeout)

    diff_proc = subprocess.run(["git", "-C", str(worktree_path), "status", "--porcelain"], capture_output=True, text=True)
    has_changes = bool(diff_proc.stdout.strip())

    journal_entry = {
        "task": task_id,
        "repo": repo_name,
        "started": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "model": model,
        "branch": branch_name,
        "status": "completed" if success else "failed",
        "reason": reason,
        "log_path": str(log_path),
        "changes_made": has_changes,
        "notes": [f"Execution {reason}"]
    }

    if success:
        if has_changes:
            subprocess.run(["git", "-C", str(worktree_path), "add", "-A"], capture_output=True)
            subprocess.run(["git", "-C", str(worktree_path), "commit", "-m", f"fix(agent): autonomous resolution for task {task_id}"], capture_output=True)
        record_journal(journal_entry, failure=False)
        CURRENT_FILE.unlink(missing_ok=True)
        cleanup_worktree(repo_path, worktree_path)
        return True, "completed"
    else:
        record_journal(journal_entry, failure=True)
        CURRENT_FILE.unlink(missing_ok=True)
        cleanup_worktree(repo_path, worktree_path)
        return False, reason

def main_loop():
    logger.info("Codex Autonomous Worker started.")
    consecutive_empty = 0

    while RUNNING:
        task_path, task_data = find_next_task()
        if not task_path:
            consecutive_empty += 1
            if consecutive_empty % 10 == 0:
                logger.info(f"Task queue empty. Waiting for tasks in {TASKS_DIR}...")
            time.sleep(CONFIG.get("poll_interval_seconds", 15))
            continue

        consecutive_empty = 0
        logger.info(f"Claiming task: {task_path.name}")
        claimed_path = mark_task_status(task_path, task_data, "claimed")

        success, reason = execute_task(claimed_path, task_data)

        if success:
            logger.info(f"Task {task_path.name} finished successfully.")
            mark_task_status(claimed_path, task_data, "done")
        else:
            logger.warning(f"Task {task_path.name} failed with reason: {reason}")
            if reason == "rate_limited":
                backoff = CONFIG.get("rate_limit_backoff_seconds", 300)
                logger.warning(f"Rate limit / cooldown detected. Backing off for {backoff}s...")
                mark_task_status(claimed_path, task_data, "pending")
                time.sleep(backoff)
            else:
                mark_task_status(claimed_path, task_data, "failed")

        time.sleep(2)

    logger.info("Codex Autonomous Worker stopped cleanly.")

if __name__ == "__main__":
    main_loop()
