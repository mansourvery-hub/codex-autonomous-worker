# Repository Guidelines

## Project Structure & Module Organization

- `src/main.rs`: Main binary entrypoint for CLI command dispatch and TUI launch.
- `src/cli.rs`: Clap-derived CLI command structures (`tui`, `loop`, `queue`, `list`, `current`, `daemon`, `logs`, `history`).
- `src/config.rs`: Configuration loader, filesystem paths, and gateway proxy settings.
- `src/campaign.rs`: Domain models, task queue parsing, session discovery, and state machine.
- `src/supervisor.rs`: 24/7 background autonomous engineering supervisor daemon and heartbeat driver loop.
- `src/tui/`: High-performance terminal control deck built on Ratatui and Crossterm (`app.rs`, `ui.rs`).
- `systemd/`: Service unit definitions for `codex-worker` and `cliproxyapi`.
- `prompts/task_prompt.md`: Standard autonomous engineering instructions (`PLAN.md` protocol, test-first fixes).

## Build, Test, and Development Commands

- `cargo build --release`: Compiles the self-contained `autopilot` binary into `target/release/autopilot`.
- `cargo test --all`: Runs all domain model, duration formatting, and configuration roundtrip tests.
- `autopilot`: Launches the keyboard-navigable Ratatui control deck.
- `autopilot loop [prompt] [--agent <codex|opencode>]`: Launches a 24/7 autonomous continuous loop campaign and opens the deck.
- `autopilot queue "prompt" [--agent <codex|opencode>]`: Queues a new continuous engineering campaign.
- `autopilot list`: Displays active, queued, and completed campaigns with color-coded status pills.
- `autopilot current`: Shows details for the currently active campaign.
- `autopilot daemon`: Runs the 24/7 background supervisor service (managed by systemd).

## Coding Style & Naming Conventions

- **Language & Style**: Idiomatic Rust 2021 edition adhering to standard rustfmt conventions.
- **Error Handling**: Use `anyhow::Result` for application-level error context and explicit enum variants for domain states.
- **Naming Patterns**: Use `snake_case` for functions and modules, `PascalCase` for structs and enums.

## Testing Guidelines

- **Validation**: Run `cargo test --all` before submitting any change.
- **Determinism**: Test all domain state transitions and duration formatters with isolated unit tests.

## Commit & Pull Request Guidelines

- **Commit Messages**: Follow Conventional Commits format (`type(scope): description`), such as `feat(core): rewrite autopilot in Rust` or `fix(tui): improve session resumption`.
- **Pull Requests**: Include a clear summary of the problem, changes made, and validation steps.
