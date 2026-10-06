# Repository Guidelines

## Project Structure & Module Organization

- `worker.py`: Core autonomous daemon supervising task execution, git worktrees, heartbeat loops, and Codex runs.
- `bin/task`: Human CLI and entrypoint for task management, live attach, and launching the control surface.
- `bin/sidebar.py`: Interactive split-screen TUI sidebar for navigating tasks and monitoring live Codex sessions.
- `prompts/task_prompt.md`: Standard instructions injected into autonomous agent runs (discovery, PLAN.md, test-first).
- `config/`: Configuration templates (e.g., `config.example.json`).
- `systemd/`: Service unit definitions for `codex-worker` and `cliproxyapi`.
- `examples/`: Starter task definition files (e.g., `examples/001-starter-task.yaml`).

## Build, Test, and Development Commands

- `./install.sh`: Sets up runtime directories, installs the `task` CLI and sidebar into `~/.local/bin`, and registers systemd units.
- `task tui` or `task`: Launches the keyboard-navigable split-screen TUI control surface (t3code-style).
- `task "prompt"`: Queues a new continuous-mode task for autonomous execution.
- `task attach`: Connects directly to the live interactive Codex TUI inside the `codex-live` tmux session.
- `task list`: Displays active, pending, and completed tasks in the queue.
- `task logs --last`: Views the most recent task execution output log.
- `python3 worker.py`: Runs the autonomous worker daemon locally for debugging.

## Coding Style & Naming Conventions

- **Language & Style**: Python 3.10+ adhering to PEP 8 standards with 4-space indentation.
- **File System Operations**: Use `pathlib.Path` instead of raw string concatenation for robust path manipulation.
- **Naming Patterns**: Use `snake_case` for functions and variables, `PascalCase` for classes, and `kebab-case` for task and configuration files.

## Testing Guidelines

- **Validation**: Verify CLI commands, TUI layout transitions, and daemon state transitions locally before submitting changes.
- **Isolation**: Test changes without polluting primary repository checkouts or production runtime spool directories (`~/srv-codex`).

## Commit & Pull Request Guidelines

- **Commit Messages**: Follow Conventional Commits format (`type(scope): description`), such as `feat(tui): add split-screen task control surface` or `feat(core): implement continuous autonomous loop`.
- **Pull Requests**: Include a clear summary of the problem, changes made, and validation steps. Always work on isolated feature branches and never commit directly to `main`.
