# Claude GUI

Desktop GUI wrapper for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI print mode (`claude -p`).

**Stack:** Tauri 2 + React + TypeScript  
**Auth:** Anthropic API key paste → OS keychain / credential store  
**Scope:** MVP — complete JSON responses (no streaming), `--resume` multi-turn, local session history.

See [`docs` in the Project store](file:///cursor/stores/self/docs/mvp-runbook.md) or ask for `mvp-runbook.md`.

## Quick start

```bash
cd ~/claude-gui   # or /home/ubuntu/claude-gui
npm install
npm run tauri dev
```

Requirements: Rust, Node 20+, Claude Code CLI on PATH (or set path in Settings), OS keyring (Keychain / Credential Manager / libsecret).

## Commands used

```bash
# first turn
claude -p "<prompt>" --output-format json --allowedTools Read

# continue
claude -p "<prompt>" --resume "<session_id>" --output-format json --allowedTools Read
```

`ANTHROPIC_API_KEY` is injected into the child process environment only.
