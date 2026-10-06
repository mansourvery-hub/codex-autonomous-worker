# Repository Guidelines

## Project Structure & Module Organization

- `worker.py`: Core 24/7 autonomous daemon supervising campaigns, git worktrees, heartbeat loops, and Codex execution.
- `bin/autopilot`: Dedicated entrypoint and CLI for autonomous campaigns, status reporting, and the TUI control deck.
- `bin/task`: Compatibility wrapper delegating directly to `bin/autopilot`.
- `bin/sidebar.py`: Interactive split-screen TUI control deck for navigating campaigns and monitoring live sessions.
- `prompts/task_prompt.md`: Standard instructions injected into autonomous agent runs (discovery, PLAN.md, test-first).
- `config/`: Configuration templates (e.g., `config.example.json`).
- `systemd/`: Service unit definitions for `codex-worker` and `cliproxyapi`.
- `examples/`: Starter campaign definition files (e.g., `examples/001-starter-task.yaml`).

## Build, Test, and Development Commands

- `./install.sh`: Sets up runtime directories, installs `autopilot` into `~/.local/bin`, and registers systemd units.
- `autopilot` or `autopilot tui`: Launches the keyboard-navigable split-screen TUI control deck.
- `autopilot queue "prompt"`: Queues a new continuous-mode campaign for autonomous execution.
- `autopilot list`: Displays active, queued, and completed campaigns.
- `autopilot current`: Shows details for the currently executing campaign.
- `autopilot logs --last`: Views the most recent campaign execution output log.
- `python3 worker.py`: Runs the autonomous supervisor daemon locally for debugging.

## Coding Style & Naming Conventions

- **Language & Style**: Python 3.10+ adhering to PEP 8 standards with 4-space indentation.
- **File System Operations**: Use `pathlib.Path` instead of raw string concatenation for robust path manipulation.
- **Naming Patterns**: Use `snake_case` for functions and variables, `PascalCase` for classes, and `kebab-case` for campaign and configuration files.

## Testing Guidelines

- **Validation**: Verify CLI commands, TUI layout transitions, and daemon state transitions locally before submitting changes.
- **Isolation**: Test changes without polluting primary repository checkouts or production runtime spool directories (`~/srv-codex`).

## Commit & Pull Request Guidelines

- **Commit Messages**: Follow Conventional Commits format (`type(scope): description`), such as `feat(tui): introduce Autopilot control deck` or `feat(core): implement continuous autonomous loop`.
- **Pull Requests**: Include a clear summary of the problem, changes made, and validation steps. Always work on isolated feature branches and never commit directly to `main`.
