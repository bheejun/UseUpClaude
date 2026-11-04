# Claude GUI

Desktop GUI wrapper for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI print mode (`claude -p`).

**Stack:** Tauri 2 + React + TypeScript  
**Auth:** Anthropic API key paste → OS keychain / credential store (never committed)  
**Scope:** MVP — complete JSON responses (no streaming), `--resume` multi-turn, local session history.

## Requirements

- Node 20+
- Rust stable (1.85+ recommended for Tauri 2)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) on `PATH` (or set the path in Settings)
- Anthropic API key (`sk-ant-…`) — paste in the app Settings UI only
- Platform WebView deps (macOS / Windows WebView2 / Linux WebKitGTK for local builds)

## Quick start

```bash
git clone https://github.com/<OWNER>/claude-gui.git
cd claude-gui
npm install
npm run tauri dev
```

Build without installers:

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

## How it talks to Claude Code

```bash
# first turn
claude -p "<prompt>" --output-format json --allowedTools Read

# continue
claude -p "<prompt>" --resume "<session_id>" --output-format json --allowedTools Read
```

`ANTHROPIC_API_KEY` is injected into the child process environment only — never written to disk by this app, never passed on argv, never committed.

## MVP defaults

| Topic | Choice |
|-------|--------|
| Auth | Anthropic API key paste only |
| Tools | Read-only (`--allowedTools Read`) |
| Streaming | Not in MVP |
| Distro | Personal sideload / local tool |

## Security notes

- Do not put API keys in `.env`, config files, or this repo.
- Session history lives in the OS app-data directory (e.g. `~/.local/share/claude-gui` on Linux).
- Keys are stored via the OS keychain / Credential Manager / linux-keyutils.
