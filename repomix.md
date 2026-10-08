# Repomix Codebase Snapshots Guide

This document explains how Repomix codebase snapshots are configured, generated, and used in this repository.

Repomix packages the repository source code, directory tree, and configuration files into structured, token-counted single-file bundles. These files are optimized for attaching or pasting directly into Web LLM chat interfaces (such as Claude.ai, ChatGPT, Gemini Advanced, and DeepSeek Chat) for research, architectural analysis, and deep debugging sessions.

---

## Snapshot Artifacts in `repomix/`

The repository automatically maintains two distinct snapshots inside the `repomix/` directory:

| Snapshot File | Approximate Size | Token Count | Primary Use Case |
|---|---|---|---|
| `repomix/repomix-src.xml` | ~140 KB | ~30,000 tokens | **Recommended for Web LLM Chat**: Contains all Rust engine code (`src/`), prompts layer (`prompts/`), manifest (`Cargo.toml`), and documentation. Leaves over 80% of context window open for reasoning. |
| `repomix/repomix-full.xml` | ~200 KB | ~48,000 tokens | **Full Repository Snapshot**: Includes tests, systemd definitions, CLI scripts, and root metadata. Ideal for frontier models with large context windows (128K+). |

Both files use the **XML** format (`<file path="...">...</file>`), which frontier LLM benchmark evaluations show provides the highest boundary clarity and fewest parsing hallucinations compared to raw markdown code fences.

---

## Using Snapshots in Web LLM Chat

1. **Download or copy** either `repomix/repomix-src.xml` or `repomix/repomix-full.xml`.
2. **Attach or paste** the file directly into your conversation with Claude, ChatGPT, Gemini, or DeepSeek.
3. **Prompt the LLM** with your specific inquiry, for example:
   - *"Analyze the PTY event loop in `src/tui/` and suggest improvements for reducing rendering latency over SSH."*
   - *"Audit `src/supervisor.rs` for race conditions in parallel task worktree management."*
   - *"Help me add a new agent engine alongside Codex and OpenCode."*

Because the snapshot includes exact file paths, line numbers, and tree structure, the web LLM will reference exact file paths and lines without hallucinating missing modules.

---

## Generating Snapshots Locally

You do not need to install anything globally. You can run Repomix on demand using `npx`:

### 1. Default Full Snapshot (Uses `repomix.config.json`)
```bash
npx --yes repomix
```
Generates `repomix/repomix-full.xml` respecting the ignore patterns defined in `repomix.config.json`.

### 2. Compact Source & Prompts Snapshot (Fastest & Smallest)
```bash
npx --yes repomix \
  --style xml \
  --output repomix/repomix-src.xml \
  --include "src/**,prompts/**,Cargo.toml,README.md,AGENTS.md,repomix.md"
```

### 3. Subsystem-Specific Debug Bundles
To package only a specific subsystem (for ultra-fast debug turns with minimal tokens):

**TUI Subsystem Only (~12,000 tokens):**
```bash
npx --yes repomix \
  --style xml \
  --output repomix/repomix-tui.xml \
  --include "src/tui/**,src/models.rs,Cargo.toml"
```

**Supervisor Daemon & Engine Only (~15,000 tokens):**
```bash
npx --yes repomix \
  --style xml \
  --output repomix/repomix-daemon.xml \
  --include "src/supervisor.rs,src/campaign.rs,src/config.rs,prompts/**,Cargo.toml"
```

### 4. Markdown Style (Alternative Format)
If you prefer standard Markdown headers (`# File: src/...`) instead of XML tags:
```bash
npx --yes repomix \
  --style markdown \
  --output repomix/repomix-src.md \
  --include "src/**,prompts/**,Cargo.toml"
```

---

## Token Budgeting Cheat Sheet

| Target LLM Model | Context Window | `repomix-src.xml` Consumption | Headroom Remaining |
|---|---|---|---|
| **Claude 3.5 Sonnet / Opus** | 200,000 tokens | ~15% | ~170,000 tokens |
| **GPT-4o / GPT-4.5** | 128,000 tokens | ~24% | ~97,000 tokens |
| **Gemini 1.5 Pro / 2.0 Flash** | 1,000,000+ tokens | ~3% | ~970,000 tokens |
| **DeepSeek V3 / R1 (Web)** | 64,000 tokens | ~48% | ~33,000 tokens |

---

## Continuous Integration (GitHub Actions)

The repository includes an automated CI workflow at `.github/workflows/repomix.yml`: 
- **On Pull Requests**: Runs Repomix, validates that no secrets or oversized build artifacts leak into the snapshot, and uploads the bundles as build artifacts.
- **On Push to `main`**: Automatically rebuilds both `repomix/repomix-full.xml` and `repomix/repomix-src.xml` and commits them back to the repository with `[skip ci]`.

This guarantees that the snapshot files in `repomix/` always reflect the latest commit on `main`.
