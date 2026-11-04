[한국어](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [Español](README.es.md)

# UseUpClaude

Desktop chat around the Claude Code CLI `-p` / `--resume` flow. A message runs `claude -p … --output-format json` once. The app shows the `result` string as markdown, not the JSON envelope. `session_id` stays inside the app for `--resume`.

> **claude -p has less context than interactive Claude Code and may not complete work as well.**

The window title and product name are UseUpClaude. The executable file is still `claude-gui`. The Rust crate name is unchanged.

## What it does

| Item | Behavior |
|------|----------|
| Chat | Bubbles, avatars, Enter to send, a working state. No token streaming |
| Reply | `result` from the CLI JSON, rendered as GFM markdown |
| Cost | This turn's `total_cost_usd` when the CLI reports it |
| Balance | A normal API key cannot read remaining credit. The console is not scraped |
| Projects | Chats grouped by working folder (cwd) |
| Existing sessions | Reads session id, folder, and a short title from `~/.claude/projects`, then `--resume`. Transcripts are not imported |
| Languages | 한국어, English, 日本語, 简体中文, Español. OS locale, otherwise Korean |
| Auth | Paste an Anthropic API key in Settings. It stays in the OS credential store and is not logged |

## Requirements

- Node.js 20+
- Rust stable (Tauri 2, 1.85+ recommended)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
- An Anthropic API key (`sk-ant-…`)
- Windows: [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)

## Run

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

`npm run tauri dev` reuses an incremental compile when a debug build already exists. For a release binary without an installer:

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

On Windows the file is `src-tauri\target\release\claude-gui.exe`.

First launch: **Settings** → save the API key → detect `claude` → **New chat** and pick a project folder.

`ANTHROPIC_API_KEY` is passed only as the child process environment. Do not put the key in argv, logs, or this repository.
