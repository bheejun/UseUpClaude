[한국어](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [Español](README.es.md)

# UseUpClaude

包住 Claude Code CLI `-p` / `--resume` 的桌面聊天。一条消息调用一次 `claude -p … --output-format json`。界面只把 `result` 渲染成 Markdown，不显示 JSON 外壳。`session_id` 只留在应用内部，供 `--resume` 使用。

> **claude -p 的上下文少于交互式 Claude Code，可能无法同样好地完成工作。**

窗口标题和产品名是 UseUpClaude。可执行文件名仍是 `claude-gui`。Rust crate 名称没有改。

## 功能

| 项目 | 行为 |
|------|------|
| 聊天 | 气泡、头像、Enter 发送、处理中状态。没有 token 流式输出 |
| 回复 | 取出 CLI JSON 的 `result`，按 GFM Markdown 渲染 |
| 费用 | CLI 返回 `total_cost_usd` 时显示这一轮的费用 |
| 余额 | 普通 API 密钥无法查询剩余额度。不会抓取控制台 |
| 项目 | 按工作文件夹 (cwd) 分组聊天 |
| 已有会话 | 从 `~/.claude/projects` 只读取会话 id、文件夹和短标题，再用 `--resume`。不导入对话原文 |
| 语言 | 한국어、English、日本語、简体中文、Español。跟随系统语言，否则为韩语 |
| 认证 | 在设置里粘贴 Anthropic API 密钥。密钥放在系统凭据存储中，不会写入日志 |

## 环境

- Node.js 20+
- Rust stable（Tauri 2，建议 1.85+）
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
- Anthropic API 密钥（`sk-ant-…`）
- Windows：[WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)

## 运行

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

如果已经有调试构建，`npm run tauri dev` 会增量编译。只要发布版可执行文件、不要安装包时：

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

Windows 上是 `src-tauri\target\release\claude-gui.exe`。

第一次打开：**设置**里保存 API 密钥 → 检测 `claude` → **新聊天**并选择项目文件夹。

`ANTHROPIC_API_KEY` 只作为子进程环境变量传入。不要把密钥放进 argv、日志或这个仓库。
