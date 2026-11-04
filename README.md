[한국어](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [Español](README.es.md)

# UseUpClaude

**Claude Code CLI의 `-p` / `--resume`를 감싸는 데스크톱 채팅.**  
메시지를 보내면 `claude -p … --output-format json`을 한 번 호출하고, 답의 `result`만 마크다운으로 보여 줍니다. 이어가기는 `--resume`입니다. `session_id`는 화면에 뿌리지 않고 앱 안에만 둡니다.

> **claude -p는 대화형 Claude Code보다 맥락이 적어서, 작업을 그만큼 잘 끝내지 못할 수 있습니다.**

창 제목과 제품 이름은 UseUpClaude입니다. 실행 파일 이름은 그대로 `claude-gui`입니다. Rust 크레이트 이름도 바꾸지 않았습니다.

## 하는 일

| 항목 | 내용 |
|------|------|
| 채팅 | 말풍선, 아바타, Enter 전송, 작업 중 표시. 스트리밍 없음 |
| 답변 | CLI JSON에서 `result`만 꺼내 GFM 마크다운으로 렌더 |
| 비용 | 턴에 `total_cost_usd`가 있으면 그 금액을 표시 |
| 잔액 | 일반 API 키로는 남은 크레딧을 조회할 수 없음. 콘솔을 긁지 않음 |
| 프로젝트 | 채팅을 작업 폴더(cwd)별로 묶음 |
| 기존 세션 | `~/.claude/projects`의 세션 id·폴더·짧은 제목만 읽고 `--resume`. 대화 원문은 가져오지 않음 |
| 언어 | 한국어, English, 日本語, 简体中文, Español. OS 로케일, 없으면 한국어 |
| 인증 | Anthropic API 키를 설정에 붙여넣기 → OS 자격 증명 저장소. 로그·저장소에 키를 넣지 않음 |

## 사전 준비

- Node.js 20+
- Rust stable (Tauri 2, 1.85+ 권장)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
- Anthropic API 키 (`sk-ant-…`)
- Windows: [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)

## 실행

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

이미 빌드된 디버그 바이너리가 있으면 프론트만 바꾼 뒤에는 `npm run tauri dev`가 증분 컴파일을 사용합니다. 릴리스 실행 파일만 만들 때:

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

Windows에서는 `src-tauri\target\release\claude-gui.exe`입니다.

첫 실행: **설정**에서 API 키 저장 → `claude` 탐지 → **새 채팅**에서 프로젝트 폴더 선택.

`ANTHROPIC_API_KEY`는 자식 프로세스 환경변수로만 넣습니다. argv, 로그, 이 저장소에 키를 넣지 마세요.
