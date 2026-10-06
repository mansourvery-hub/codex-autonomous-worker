#!/usr/bin/env python3
import os
import sys
import time
import json
import yaml
import glob
import curses
import subprocess
from pathlib import Path

BASE_DIR = Path("/home/ubuntu/srv-codex")
TASKS_DIR = BASE_DIR / "tasks"
STATE_DIR = BASE_DIR / "state"
LOGS_DIR = BASE_DIR / "logs"
CURRENT_FILE = STATE_DIR / "current.json"

def get_tasks():
    task_files = sorted(list(TASKS_DIR.glob("*.yaml")) + list(TASKS_DIR.glob("*.yml")) + list(TASKS_DIR.glob("*.json")))
    current_data = {}
    if CURRENT_FILE.exists():
        try:
            current_data = json.loads(CURRENT_FILE.read_text())
        except Exception:
            pass

    tasks = []
    for p in task_files:
        status = "pending"
        for s in ["claimed", "done", "failed"]:
            if f".{s}." in p.name or p.name.endswith(f".{s}.yaml") or p.name.endswith(f".{s}.json"):
                status = s
                break

        # Check if currently executing
        task_id = p.stem.split(".")[0].split("-")[0]
        if current_data.get("task_id") == task_id or current_data.get("task_id") == p.stem.split(".")[0]:
            status = "running"

        data = {}
        try:
            with open(p, "r") as f:
                data = yaml.safe_load(f) if p.suffix in [".yaml", ".yml"] else json.load(f)
        except Exception:
            pass

        tasks.append({
            "id": data.get("id", task_id),
            "repo": data.get("repo", "unknown"),
            "status": status,
            "mode": data.get("mode", "continuous"),
            "model": data.get("model", "default"),
            "prompt": data.get("prompt", p.stem),
            "file": p
        })

    # Sort: running first, then claimed, pending, done, failed
    order = {"running": 0, "claimed": 1, "pending": 2, "done": 3, "failed": 4}
    tasks.sort(key=lambda t: (order.get(t["status"], 5), t["id"]))
    return tasks, current_data

def find_task_log(task_id):
    logs = sorted(LOGS_DIR.glob(f"*{task_id}*.log"), key=os.path.getmtime)
    if not logs:
        logs = sorted(LOGS_DIR.glob("codex_*.log"), key=os.path.getmtime)
    return logs[-1] if logs else None

def action_select_task(task):
    status = task["status"]
    task_id = task["id"]

    if status in ["running", "claimed"]:
        # Attach right pane to codex-live
        subprocess.run(["tmux", "respawn-pane", "-k", "-t", "codex-tui:0.1", "TMUX= tmux attach -t codex-live"], capture_output=True)
        subprocess.run(["tmux", "select-pane", "-t", "codex-tui:0.1"], capture_output=True)
    elif status in ["done", "failed"]:
        log_f = find_task_log(task_id)
        if log_f:
            subprocess.run(["tmux", "respawn-pane", "-k", "-t", "codex-tui:0.1", f"tail -n 80 -f {log_f}"], capture_output=True)
        else:
            subprocess.run(["tmux", "respawn-pane", "-k", "-t", "codex-tui:0.1", f"echo 'Task #{task_id} completed. No log file found.'"], capture_output=True)
        subprocess.run(["tmux", "select-pane", "-t", "codex-tui:0.1"], capture_output=True)
    else:
        # Pending task: show details card
        card_cmd = f"sh -c 'echo Task #{task_id} [PENDING]; echo Waiting for worker daemon...; sleep 10'"
        subprocess.run(["tmux", "respawn-pane", "-k", "-t", "codex-tui:0.1", card_cmd], capture_output=True)

def modal_input(stdscr, title, prompt_label):
    h, w = stdscr.getmaxyx()
    curses.echo()
    curses.curs_set(1)
    stdscr.nodelay(False)

    box_h = 5
    box_w = max(20, w - 4)
    start_y = max(1, h // 2 - 2)
    start_x = 2

    win = curses.newwin(box_h, box_w, start_y, start_x)
    win.box()
    win.addstr(1, 2, f" {title} "[:box_w - 4], curses.A_BOLD)
    win.addstr(2, 2, f"{prompt_label}: "[:box_w - 4])
    win.refresh()

    user_input = ""
    try:
        user_input = win.getstr(3, 2, box_w - 6).decode("utf-8").strip()
    except Exception:
        pass

    curses.noecho()
    curses.curs_set(0)
    stdscr.nodelay(True)
    return user_input

def queue_new_task(prompt_text):
    if not prompt_text:
        return
    existing = glob.glob(str(TASKS_DIR / "*.yaml")) + glob.glob(str(TASKS_DIR / "*.json"))
    nums = []
    for f in existing:
        base = Path(f).name.split(".")[0].split("-")[0]
        if base.isdigit():
            nums.append(int(base))
    task_id = f"{(max(nums) + 1 if nums else 1):03d}"
    filename = f"{task_id}-feature.yaml"
    task_file = TASKS_DIR / filename
    data = {
        "id": task_id,
        "repo": "codex-autonomous-worker",
        "type": "feature",
        "mode": "continuous",
        "priority": "medium",
        "status": "pending",
        "model": "agentrouter/deepseek-v4-flash",
        "prompt": prompt_text
    }
    with open(task_file, "w") as f:
        yaml.dump(data, f, default_flow_style=False)

def steer_running_task(steer_text):
    if not steer_text:
        return
    escaped = steer_text.replace("'", "'''")
    subprocess.run(["tmux", "send-keys", "-t", "codex-live", escaped, "C-m"], capture_output=True)

def main(stdscr):
    curses.start_color()
    curses.use_default_colors()
    curses.curs_set(0)
    stdscr.nodelay(True)
    stdscr.timeout(1000)

    # Initialize color pairs
    curses.init_pair(1, curses.COLOR_GREEN, -1)   # Running / Done
    curses.init_pair(2, curses.COLOR_YELLOW, -1)  # Claimed / Active
    curses.init_pair(3, curses.COLOR_CYAN, -1)    # Pending / Header
    curses.init_pair(4, curses.COLOR_RED, -1)     # Failed
    curses.init_pair(5, curses.COLOR_BLACK, curses.COLOR_CYAN) # Selection highlight
    curses.init_pair(6, curses.COLOR_WHITE, -1)   # Normal

    selected_idx = 0
    message = ""
    message_time = 0

    while True:
        h, w = stdscr.getmaxyx()
        stdscr.erase()

        tasks, current_data = get_tasks()
        if selected_idx >= len(tasks):
            selected_idx = max(0, len(tasks) - 1)

        # Header
        title = " CODEX TASKS (t3code) "
        stdscr.addstr(0, 0, title[:w], curses.color_pair(3) | curses.A_BOLD)
        
        # Summary counts
        running_cnt = sum(1 for t in tasks if t["status"] == "running")
        pending_cnt = sum(1 for t in tasks if t["status"] == "pending")
        done_cnt = sum(1 for t in tasks if t["status"] == "done")
        summary_str = f"● {running_cnt} Run  ○ {pending_cnt} Pnd  ✔ {done_cnt} Done"
        stdscr.addstr(1, 0, summary_str[:w], curses.color_pair(6))
        stdscr.addstr(2, 0, ("─" * (w - 1))[:w], curses.color_pair(3))

        # Task list area
        list_h = max(1, h - 8)
        start_y = 3

        if not tasks:
            stdscr.addstr(start_y, 1, "(No tasks queued)", curses.color_pair(6))
            stdscr.addstr(start_y + 1, 1, "Press 'n' to add one.", curses.color_pair(3))
        else:
            scroll_offset = max(0, selected_idx - list_h + 1) if selected_idx >= list_h else 0
            for i in range(list_h):
                t_idx = scroll_offset + i
                if t_idx >= len(tasks):
                    break
                t = tasks[t_idx]
                is_sel = (t_idx == selected_idx)
                row_y = start_y + i

                badge = "[?]"
                color = curses.color_pair(6)
                if t["status"] == "running":
                    badge = "[●RUN]"
                    color = curses.color_pair(1) | curses.A_BOLD
                elif t["status"] == "claimed":
                    badge = "[▶CLM]"
                    color = curses.color_pair(2)
                elif t["status"] == "pending":
                    badge = "[○PND]"
                    color = curses.color_pair(3)
                elif t["status"] == "done":
                    badge = "[✔DON]"
                    color = curses.color_pair(1)
                elif t["status"] == "failed":
                    badge = "[✖ERR]"
                    color = curses.color_pair(4)

                prefix = "▸ " if is_sel else "  "
                line = f"{prefix}{badge} #{t['id']} {t['repo']}"
                if is_sel:
                    stdscr.addstr(row_y, 0, line[:w].ljust(w), curses.color_pair(5) | curses.A_BOLD)
                else:
                    stdscr.addstr(row_y, 0, prefix[:w], curses.color_pair(6))
                    stdscr.addstr(row_y, len(prefix), badge[:w - len(prefix)], color)
                    rem = f" #{t['id']} {t['repo']}"
                    stdscr.addstr(row_y, len(prefix) + len(badge), rem[:max(0, w - len(prefix) - len(badge))], curses.color_pair(6))

        # Footer divider and actions
        footer_y = max(3, h - 5)
        stdscr.addstr(footer_y, 0, ("─" * (w - 1))[:w], curses.color_pair(3))

        if message and time.time() - message_time < 3:
            stdscr.addstr(footer_y + 1, 0, f"★ {message}"[:w], curses.color_pair(2) | curses.A_BOLD)
        else:
            stdscr.addstr(footer_y + 1, 0, "[Enter] Focus Codex"[:w], curses.color_pair(1) | curses.A_BOLD)
            stdscr.addstr(footer_y + 2, 0, "[F6/C-w] Focus Sidebar"[:w], curses.color_pair(3))
            stdscr.addstr(footer_y + 3, 0, "[n] New  [s] Steer  [q] Q"[:w], curses.color_pair(6))

        stdscr.refresh()

        try:
            ch = stdscr.getch()
        except curses.error:
            ch = -1

        if ch in [curses.KEY_UP, ord('k')]:
            selected_idx = max(0, selected_idx - 1)
        elif ch in [curses.KEY_DOWN, ord('j')]:
            if tasks:
                selected_idx = min(len(tasks) - 1, selected_idx + 1)
        elif ch in [10, 13, curses.KEY_ENTER]:
            if tasks and selected_idx < len(tasks):
                action_select_task(tasks[selected_idx])
        elif ch == ord('n'):
            p_text = modal_input(stdscr, "Queue New Task", "Prompt")
            if p_text:
                queue_new_task(p_text)
                message = "Task queued!"
                message_time = time.time()
        elif ch == ord('s'):
            s_text = modal_input(stdscr, "Steer Running Task", "Correction")
            if s_text:
                steer_running_task(s_text)
                message = "Steering sent!"
                message_time = time.time()
        elif ch == ord('q'):
            subprocess.run(["tmux", "detach-client"], capture_output=True)
            break

if __name__ == "__main__":
    curses.wrapper(main)

