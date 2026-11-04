[한국어](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [Español](README.es.md)

# UseUpClaude

Claude Code CLI の `-p` / `--resume` を包むデスクトップチャットです。メッセージは `claude -p … --output-format json` を1回呼び、JSON 全体ではなく `result` をマークダウンで表示します。`session_id` は `--resume` 用にアプリ内だけに残します。

> **claude -p は対話型の Claude Code より文脈が少なく、作業を同じようには完了できないことがあります。**

ウィンドウ名と製品名は UseUpClaude です。実行ファイル名は `claude-gui` のままです。Rust クレート名も変えていません。

## できること

| 項目 | 内容 |
|------|------|
| チャット | 吹き出し、アバター、Enter 送信、作業中表示。トークンのストリーミングはなし |
| 返信 | CLI JSON の `result` を GFM マークダウンで表示 |
| 費用 | `total_cost_usd` があるターンだけその金額 |
| 残高 | 通常の API キーでは残りクレジットを読めません。コンソールはスクレイプしません |
| プロジェクト | チャットを作業フォルダ (cwd) ごとにまとめます |
| 既存セッション | `~/.claude/projects` からセッション id・フォルダ・短いタイトルだけ読み、`--resume`。本文は取り込みません |
| 言語 | 한국어, English, 日本語, 简体中文, Español。OS のロケール、なければ韓国語 |
| 認証 | 設定に Anthropic API キーを貼る。OS の資格情報ストアに置き、ログには出しません |

## 必要なもの

- Node.js 20+
- Rust stable（Tauri 2、1.85+ 推奨）
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
- Anthropic API キー（`sk-ant-…`）
- Windows: [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)

## 起動

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

デバッグビルドがすでにあるときは `npm run tauri dev` が差分コンパイルを使います。インストーラなしのリリースバイナリ:

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

Windows では `src-tauri\target\release\claude-gui.exe` です。

初回: **設定**で API キーを保存 → `claude` を検出 → **新しいチャット**でプロジェクトフォルダを選ぶ。

`ANTHROPIC_API_KEY` は子プロセスの環境変数にだけ渡します。argv、ログ、このリポジトリにキーを置かないでください。
