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
import urllib.request
import urllib.error

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

# Setup logging
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

RUNNING = True

def handle_sigterm(signum, frame):
    global RUNNING
    logger.info("Termination signal received. Gracefully exiting after current iteration...")
    RUNNING = False

signal.signal(signal.SIGINT, handle_sigterm)
signal.signal(signal.SIGTERM, handle_sigterm)

def check_cliproxy_health(cliproxy_url):
    try:
        req = urllib.request.Request(f"{cliproxy_url}/models", headers={
            "User-Agent": "codex-worker",
            "Authorization": "Bearer sk-JxARBmEuH2dxVTsmhEtJ0uOBJUTUMxb7UAcIU8TfxNBve3ZK"
        })
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status == 200
    except Exception as e:
        logger.debug(f"CLIProxyAPI health check note: {e}")
        return False

def record_journal(entry, failure=False):
    target = FAILURES_FILE if failure else HISTORY_FILE
    with open(target, "a") as f:
        f.write(json.dumps(entry) + "\n")

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
                if path.suffix in [".yaml", ".yml"]:
                    data = yaml.safe_load(f)
                else:
                    data = json.load(f)
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
        logger.info(f"Worktree path {worktree_path} already exists. Cleaning up...")
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
    task_log_file = LOGS_DIR / f"codex_exec_{int(time.time())}.log"
    exit_code_file = LOGS_DIR / f"codex_exit_{int(time.time())}.status"
    cliproxy_info = CONFIG.get("cliproxy", {})
    cliproxy_url = cliproxy_info.get("url", "http://127.0.0.1:8317/v1")
    catalog_json = cliproxy_info.get("catalog_json", "/home/ubuntu/.codex/model-catalogs/gateway.json")

    prompt_file = worktree_path / ".agent_prompt.txt"
    prompt_file.write_text(full_prompt)

    runner_script = worktree_path / ".run_agent.sh"
    cmd_body = [
        "#!/bin/bash",
        f'codex exec -C "{worktree_path}" -c openai_base_url="{cliproxy_url}" -c model="{model}" -c model_catalog_json="{catalog_json}" --color always --sandbox workspace-write --dangerously-bypass-approvals-and-sandbox < "{prompt_file}" 2>&1 | tee "{task_log_file}"',
        f'echo $? > "{exit_code_file}"'
    ]
    runner_script.write_text(chr(10).join(cmd_body) + chr(10))
    runner_script.chmod(0o755)

    logger.info(f"Starting live Codex agent in tmux session 'codex-live' with model {model} (timeout={timeout_secs}s)")
    start_time = time.time()
    subprocess.run(["tmux", "kill-session", "-t", "codex-live"], capture_output=True)
    res = subprocess.run(["tmux", "new-session", "-d", "-s", "codex-live", "-c", str(worktree_path), str(runner_script)], capture_output=True, text=True)
    if res.returncode != 0:
        logger.error(f"Failed to start tmux session: {res.stderr}")
        return False, f"tmux_error: {res.stderr}", task_log_file

    exit_code = None
    while time.time() - start_time < timeout_secs:
        if exit_code_file.exists():
            try:
                exit_code = int(exit_code_file.read_text().strip())
                break
            except Exception:
                pass
        time.sleep(1)

    elapsed = time.time() - start_time
    exit_code_file.unlink(missing_ok=True)
    runner_script.unlink(missing_ok=True)
    prompt_file.unlink(missing_ok=True)

    if exit_code is None:
        logger.error("Task timed out. Killing tmux session 'codex-live'...")
        subprocess.run(["tmux", "kill-session", "-t", "codex-live"], capture_output=True)
        return False, "timeout", task_log_file

    subprocess.run(["tmux", "kill-session", "-t", "codex-live"], capture_output=True)

    if exit_code == 0:
        logger.info(f"Codex completed successfully in {elapsed:.1f}s")
        return True, "completed", task_log_file
    else:
        logger.warning(f"Codex exited with code {exit_code} in {elapsed:.1f}s")
        if task_log_file.exists():
            with open(task_log_file, "r") as f:
                log_tail = f.read()[-2000:]
            if any(k in log_tail.lower() for k in ["429", "rate limit", "quota", "overloaded", "cooling"]):
                return False, "rate_limited", task_log_file
        return False, f"exit_code_{exit_code}", task_log_file

def execute_task(task_file, task_data):
    task_id = str(task_data.get("id", task_file.stem))
    repo_name = task_data.get("repo")
    prompt = task_data.get("prompt", "")
    model = task_data.get("model", CONFIG.get("default_model", "gemini-3.5-flash-lite"))
    branch_name = f"agent/task-{task_id}-{int(time.time())}"
    worktree_path = WORKTREES_DIR / f"task-{task_id}"

    # Find repo
    repo_path = REPOS_DIR / repo_name
    if not repo_path.exists():
        cand = Path(f"/home/ubuntu/github-projects/{repo_name}")
        if cand.exists():
            repo_path = cand
    if not repo_path.exists():
        logger.error(f"Repo {repo_name} not found at {repo_path} or /home/ubuntu/github-projects/{repo_name}")
        return False, "repo_not_found"

    # Build prompt
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

