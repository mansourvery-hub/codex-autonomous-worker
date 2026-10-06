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
TMUX_SESSION_NAME = "codex-live"
TUI_SESSION_NAME = "autopilot-deck"

def format_duration(seconds):
    seconds = int(seconds)
    if seconds < 60:
        return f"{seconds}s"
    minutes = seconds // 60
    if minutes < 60:
        return f"{minutes}m"
    hours = minutes // 60
    rem_min = minutes % 60
    return f"{hours}h {rem_min:02d}m"

def get_tasks():
    task_files = sorted(list(TASKS_DIR.glob("*.yaml")) + list(TASKS_DIR.glob("*.yml")) + list(TASKS_DIR.glob("*.json")))
    current_data = {}
    if CURRENT_FILE.exists():
        try:
            current_data = json.loads(CURRENT_FILE.read_text())
        except Exception:
            pass

    tasks = []
    now = time.time()
    for p in task_files:
        status = "pending"
        for s in ["claimed", "done", "failed"]:
            if f".{s}." in p.name or p.name.endswith(f".{s}.yaml") or p.name.endswith(f".{s}.json"):
                status = s
                break

        task_id = p.stem.split(".")[0].split("-")[0]
        is_active = (current_data.get("task_id") == task_id or current_data.get("task_id") == p.stem.split(".")[0])
        if is_active:
            status = "running"

        data = {}
        try:
            with open(p, "r") as f:
                data = yaml.safe_load(f) if p.suffix in [".yaml", ".yml"] else json.load(f)
        except Exception:
            pass

        mtime = os.path.getmtime(p)
        age_str = format_duration(now - mtime) + " ago"

        tasks.append({
            "id": data.get("id", task_id),
            "repo": data.get("repo", "unknown"),
            "status": status,
            "mode": data.get("mode", "continuous"),
            "iteration": current_data.get("iteration", 1) if is_active else data.get("iterations", 15),
            "max_iterations": data.get("iterations", 15),
            "branch": current_data.get("branch", f"agent/task-{task_id}") if is_active else f"agent/task-{task_id}",
            "model": data.get("model", "deepseek-v4"),
            "prompt": data.get("prompt", p.stem).strip(),
            "age": age_str,
            "file": p
        })

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
        subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", f"TMUX= tmux attach -t {TMUX_SESSION_NAME}"], capture_output=True)
        subprocess.run(["tmux", "select-pane", "-t", f"{TUI_SESSION_NAME}:0.1"], capture_output=True)
    elif status in ["done", "failed"]:
        log_f = find_task_log(task_id)
        if log_f:
            subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", f"tail -n 120 -f {log_f}"], capture_output=True)
        else:
            subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", f"echo 'Campaign #{task_id} completed. No log file available.'"], capture_output=True)
        subprocess.run(["tmux", "select-pane", "-t", f"{TUI_SESSION_NAME}:0.1"], capture_output=True)
    else:
        # Pending task: show details in right pane without capturing focus
        info_cmd = f"sh -c 'echo Campaign #{task_id} [QUEUED]; echo Standing by for autonomous daemon...; sleep 10'"
        subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", info_cmd], capture_output=True)

def preview_task(task):
    status = task["status"]
    task_id = task["id"]
    if status in ["running", "claimed"]:
        # Attach to live stream
        subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", f"TMUX= tmux attach -t {TMUX_SESSION_NAME}"], capture_output=True)
    elif status in ["done", "failed"]:
        log_f = find_task_log(task_id)
        if log_f:
            subprocess.run(["tmux", "respawn-pane", "-k", "-t", f"{TUI_SESSION_NAME}:0.1", f"tail -n 80 -f {log_f}"], capture_output=True)

def modal_input(stdscr, title, prompt_label):
    h, w = stdscr.getmaxyx()
    curses.echo()
    curses.curs_set(1)
    stdscr.nodelay(False)

    box_h = 5
    box_w = max(25, min(w - 4, 60))
    start_y = max(1, h // 2 - 2)
    start_x = max(1, (w - box_w) // 2)

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

def queue_new_campaign(prompt_text):
    if not prompt_text:
        return
    existing = glob.glob(str(TASKS_DIR / "*.yaml")) + glob.glob(str(TASKS_DIR / "*.json"))
    nums = []
    for f in existing:
        base = Path(f).name.split(".")[0].split("-")[0]
        if base.isdigit():
            nums.append(int(base))
    task_id = f"{(max(nums) + 1 if nums else 1):03d}"
    filename = f"{task_id}-campaign.yaml"
    task_file = TASKS_DIR / filename
    data = {
        "id": task_id,
        "repo": "codex-autonomous-worker",
        "type": "feature",
        "mode": "continuous",
        "iterations": 20,
        "priority": "high",
        "status": "pending",
        "model": "agentrouter/deepseek-v4-flash",
        "prompt": prompt_text
    }
    with open(task_file, "w") as f:
        yaml.dump(data, f, default_flow_style=False)

def main(stdscr):
    curses.start_color()
    curses.use_default_colors()
    curses.curs_set(0)
    curses.mousemask(curses.ALL_MOUSE_EVENTS)
    stdscr.nodelay(True)
    stdscr.timeout(1000)

    # Initialize color palette
    curses.init_pair(1, curses.COLOR_GREEN, -1)   # Working / Done
    curses.init_pair(2, curses.COLOR_YELLOW, -1)  # Claimed / Active
    curses.init_pair(3, curses.COLOR_CYAN, -1)    # Headers & Borders
    curses.init_pair(4, curses.COLOR_RED, -1)     # Error / Failed
    curses.init_pair(5, curses.COLOR_BLACK, curses.COLOR_CYAN) # Card selection highlight
    curses.init_pair(6, curses.COLOR_WHITE, -1)   # Normal body
    curses.init_pair(7, curses.COLOR_MAGENTA, -1) # Queued / Subtitle

    selected_idx = 0
    last_previewed_idx = -1
    message = ""
    message_time = 0

    while True:
        h, w = stdscr.getmaxyx()
        stdscr.erase()

        tasks, current_data = get_tasks()
        if selected_idx >= len(tasks):
            selected_idx = max(0, len(tasks) - 1)

        # 1. Top 24/7 Daemon Banner
        title_banner = " AUTOPILOT CONTROL DECK "
        stdscr.addstr(0, 0, title_banner[:w], curses.color_pair(3) | curses.A_BOLD)
        
        running_cnt = sum(1 for t in tasks if t["status"] == "running")
        pending_cnt = sum(1 for t in tasks if t["status"] == "pending")
        done_cnt = sum(1 for t in tasks if t["status"] == "done")

        daemon_status = "[● 24/7 DAEMON: ACTIVE]" if running_cnt > 0 else "[○ DAEMON: STANDBY]"
        daemon_color = curses.color_pair(1) if running_cnt > 0 else curses.color_pair(7)
        stdscr.addstr(1, 0, daemon_status[:w], daemon_color | curses.A_BOLD)

        metrics_line = f"Loop: {running_cnt} Active · {pending_cnt} Queued · {done_cnt} Done"
        stdscr.addstr(2, 0, metrics_line[:w], curses.color_pair(6))
        stdscr.addstr(3, 0, ("─" * (w - 1))[:w], curses.color_pair(3))

        # 2. Rich Multi-Line Card Area
        # Each card takes 3 lines + 1 separator = 4 lines total
        card_height = 4
        start_y = 4
        available_height = max(4, h - 8)
        max_visible_cards = max(1, available_height // card_height)

        scroll_offset = max(0, selected_idx - max_visible_cards + 1) if selected_idx >= max_visible_cards else 0

        if not tasks:
            stdscr.addstr(start_y + 1, 2, "No autonomous campaigns active.", curses.color_pair(6))
            stdscr.addstr(start_y + 2, 2, "Press [n] to launch a new 24/7 campaign.", curses.color_pair(3))
        else:
            for i in range(max_visible_cards):
                t_idx = scroll_offset + i
                if t_idx >= len(tasks):
                    break
                t = tasks[t_idx]
                is_sel = (t_idx == selected_idx)
                card_y = start_y + (i * card_height)

                # Determine status pill
                if t["status"] == "running":
                    pill = "[● WORKING]"
                    p_color = curses.color_pair(1) | curses.A_BOLD
                elif t["status"] == "claimed":
                    pill = "[▶ CLAIMED]"
                    p_color = curses.color_pair(2) | curses.A_BOLD
                elif t["status"] == "pending":
                    pill = "[○ QUEUED ]"
                    p_color = curses.color_pair(7)
                elif t["status"] == "done":
                    pill = "[✔ DONE   ]"
                    p_color = curses.color_pair(1)
                else:
                    pill = "[✖ ERROR  ]"
                    p_color = curses.color_pair(4)

                prefix = "▸ " if is_sel else "  "

                # Line 1: Header row: Pill + Task ID + Age
                l1 = f"{prefix}{pill} #{t['id']} · {t['age']}"
                if is_sel:
                    stdscr.addstr(card_y, 0, l1[:w].ljust(w), curses.color_pair(5) | curses.A_BOLD)
                else:
                    stdscr.addstr(card_y, 0, prefix[:w], curses.color_pair(6))
                    stdscr.addstr(card_y, len(prefix), pill[:max(0, w - len(prefix))], p_color)
                    suffix = f" #{t['id']} · {t['age']}"
                    stdscr.addstr(card_y, len(prefix) + len(pill), suffix[:max(0, w - len(prefix) - len(pill))], curses.color_pair(6))

                # Line 2: Task Title / Prompt
                l2 = f"    {t['prompt']}"
                stdscr.addstr(card_y + 1, 0, l2[:w].ljust(w) if is_sel else l2[:w], curses.color_pair(6) | (curses.A_BOLD if is_sel else 0))

                # Line 3: Repo + Context + Loop metrics
                if t["status"] == "running":
                    l3 = f"    {t['repo']} · {t['branch']} · Iter #{t['iteration']}/{t['max_iterations']}"
                else:
                    l3 = f"    {t['repo']} · {t['model']} · {t['mode']} loop"
                stdscr.addstr(card_y + 2, 0, l3[:w].ljust(w) if is_sel else l3[:w], curses.color_pair(3))

                # Line 4: Subtle separator between cards
                stdscr.addstr(card_y + 3, 0, (" " * w) if is_sel else ("·" * min(w - 1, 30)), curses.color_pair(3))

        # 3. Footer Bar
        footer_y = max(4, h - 4)
        stdscr.addstr(footer_y, 0, ("─" * (w - 1))[:w], curses.color_pair(3))

        if message and time.time() - message_time < 3:
            stdscr.addstr(footer_y + 1, 0, f"★ {message}"[:w], curses.color_pair(2) | curses.A_BOLD)
        else:
            stdscr.addstr(footer_y + 1, 0, "[Enter] Talk / Interact with Agent"[:w], curses.color_pair(1) | curses.A_BOLD)
            stdscr.addstr(footer_y + 2, 0, "[Tab/F6] Focus Sidebar · [n] New · [q] Detach"[:w], curses.color_pair(3))

        stdscr.refresh()

        # Update preview in right pane if selection changed
        if tasks and selected_idx != last_previewed_idx:
            preview_task(tasks[selected_idx])
            last_previewed_idx = selected_idx

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
            p_text = modal_input(stdscr, "New 24/7 Autonomous Campaign", "Objective")
            if p_text:
                queue_new_campaign(p_text)
                message = "Campaign queued!"
                message_time = time.time()
                last_previewed_idx = -1
        elif ch == ord('q'):
            subprocess.run(["tmux", "detach-client"], capture_output=True)
            break
        elif ch == curses.KEY_MOUSE:
            try:
                _, mx, my, _, _ = curses.getmouse()
                if my >= start_y:
                    clicked_idx = scroll_offset + ((my - start_y) // card_height)
                    if 0 <= clicked_idx < len(tasks):
                        selected_idx = clicked_idx
            except Exception:
                pass

if __name__ == "__main__":
    curses.wrapper(main)
