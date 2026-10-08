# Codex Autonomous Worker

An always-on, self-supervised autonomous engineering daemon that uses OpenAI Codex (`codex exec`) as a disposable worker process, supervised by `systemd`, backed by isolated Git worktrees, persistent journaling, and multi-tier rate limit mitigation.

---

## The Core Concept

Codex should not be responsible for staying alive 24/7. 

Instead of running a single fragile agent session that dies when SSH drops, hits context exhaustion, or halts on rate limits:
- **`systemd`** supervises the worker daemon.
- **The worker daemon** launches bounded, disposable `codex exec` tasks.
- **Isolated Git worktrees** ensure your primary repository checkouts are never polluted.
- **`CLIProxyAPI` + Worker Backoff** protects quota and automatically cools down exhausted accounts.
- **Live Terminal Attachment**: Tasks run inside persistent `tmux` sessions so you can attach directly to the live agent TUI at any time with `task attach`.

```text
systemd
 ├── cliproxyapi.service (:8317) ──> rotates model keys, handles quotas/cooldowns
 │
 └── codex-worker.service
       ├── scans ~/srv-codex/tasks/
       ├── claims pending task (.claimed)
       ├── creates isolated git worktree & branch in ~/srv-codex/worktrees/
       ├── runs bounded 'codex exec' with safety and timeouts inside tmux (codex-live)
       ├── tests, reviews, implements minimal fixes
       ├── commits changes
       ├── journals findings into history.jsonl
       ├── handles 429/quota backoff gracefully without hammering
       └── moves to next task
```

---

## Features

- **Continuous 24/7 Operation**: Survives SSH disconnects, terminal closures, reboots, and network resets.
- **Branch & Worktree Isolation**: Agents work strictly on disposable branches (`agent/task-...`) inside isolated worktrees. Main/master remains pristine.
- **Live Observation (`task attach`)**: Watch the agent think, run tools, and execute commands in full color in real time. Detach anytime with `Ctrl-b d`.
- **Session Resumption (`task resume`)**: Step into the interactive Codex TUI with the full session loaded after a run finishes.
- **Mistake Memory**: Records failed attempts in `failures.jsonl` and injects past failure notes into subsequent task prompts so the agent never repeats dead ends.
- **Two-Tier Rate Limit Resilience**:
  1. *Proxy level*: Rotates across multiple accounts and providers with automatic cooldowns.
  2. *Worker level*: Detects HTTP 429 or quota exhaustion, pauses for backoff (default 5m), re-queues the task, and resumes automatically.
- **Zero Config Human CLI (`task`)**: Queue tasks in plain English, check queue status, view history, and inspect logs.

---

## Directory Architecture

```text
~/codex-worker/
 ├── worker.py                 # Core supervisor daemon
 ├── config.json               # Daemon configuration
 └── task_prompt.md            # Standard autonomous engineering instructions

~/srv-codex/
 ├── tasks/                    # Task queue (.yaml / .json files)
 ├── repos/                    # Target repo pointers (symlinks or checkouts)
 ├── worktrees/                # Ephemeral per-task workspaces
 ├── state/                    # history.jsonl, failures.jsonl, current.json
 └── logs/                     # worker.log and per-run codex_exec_*.log

~/.local/bin/
 └── task                      # CLI management tool
```

---

## Installation

### Prerequisites

- Linux (Ubuntu 22.04+ or Debian recommended)
- Python 3.10+ with `pyyaml` (`pip install pyyaml`)
- `git`, `tmux`, `curl`, `jq`
- [Codex CLI](https://github.com/openai/codex) installed
- (Optional but recommended) [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) for free model pooling and failover

### One-Command Setup

```bash
git clone https://github.com/mansourvery-hub/codex-autonomous-worker.git
cd codex-autonomous-worker
./install.sh
```

The script creates the runtime directories, installs `task` into `~/.local/bin/`, configures systemd units, and starts the services.

---

## Usage

### 1. The Autopilot Control Deck (TUI)

Launch the dedicated two-column terminal control surface:

```bash
# Launch the interactive split control deck over SSH:
autopilot
```

- **Left Pane (~42% width)**: Shows 24/7 daemon status and rich campaign cards (status pill, runtime, objective, branch, iteration counter, and PLAN checklist progress).
- **Right Pane (~58% width)**: Live interactive agent workspace. Press `Enter` on an active campaign to jump into the session and talk directly to Codex.
- **Navigation**: Use `Up`/`Down` or `j`/`k` to browse. Press `Tab`, `F6`, `Alt-Left`, or click the sidebar with your mouse to return to the cards. Press `n` to queue a new campaign. Press `q` to detach.

### 2. Queueing 24/7 Autonomous Campaigns

Queue long-running iterative campaigns from any directory:

```bash
# Queue a continuous multi-iteration engineering campaign (defaults to Codex):
autopilot queue "Audit timezone handling in review_history, add regression tests, and fix edge cases"

# Run with OpenCode instead of Codex:
autopilot queue "Audit domain models and run verification" --agent opencode

# Specify explicit repository, model, and iteration budget:
autopilot queue "Harden authentication flow and migrations" -r my-app -m agentrouter/deepseek-v4-flash --iterations 25
```

### 3. CLI Management

```bash
# Check active campaign details:
autopilot current

# List pending and completed campaigns:
autopilot list

# Inspect recent campaign history and notes:
autopilot history

# Stream daemon logs:
autopilot logs -f

# View full output from the latest agent execution:
autopilot logs --last
```

---

## Manual Task Queueing

If you prefer configuration files over the CLI, drop a YAML file into `~/srv-codex/tasks/001-my-task.yaml`:

```yaml
id: "001"
repo: chess-repertoire-srs
type: bugfix
priority: high
status: pending
prompt: |
  Investigate intermittent authentication failure.
  Reproduce with a unit test, implement minimal fix, and commit.
```

The daemon detects the file within 15 seconds, claims it, creates the isolated worktree, runs Codex, and moves the task to `.done`.

---

## Service Management

```bash
# Status
sudo systemctl status codex-worker.service
sudo systemctl status cliproxyapi.service

# Restart
sudo systemctl restart codex-worker.service

# Logs via systemd journal
journalctl -u codex-worker.service -f
```

---

## Support

Building small, useful software — if this project saves you time, consider [buying me a coffee](https://buymeacoffee.com/cassandre60) to help keep it maintained.

<a href="https://buymeacoffee.com/cassandre60"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" width="150" alt="Buy Me A Coffee"></a>

## License

MIT

