# UseUpClaude

**Claude Code CLI의 `-p` / `--resume`를 감싸는 데스크톱 GUI.**  
채팅하듯 메시지를 보내면 내부에서 `claude -p … --output-format json`을 호출하고, 이어가기는 `--resume <session_id>`로 처리합니다.

> English: A local Tauri 2 + React desktop client that wraps Claude Code print mode (`claude -p`) with multi-turn chat UX via `--resume`. Paste your Anthropic API key in Settings — no Claude console login scraping.

## 이게 뭔가요

| 항목 | 내용 |
|------|------|
| 하는 일 | GUI 채팅 → `claude -p` 원샷 호출 + 로컬 세션/히스토리 |
| 이어가기 | CLI가 준 `session_id`로 `--resume` (프롬프트 재조립이 1순위가 아님) |
| 인증 | **Anthropic API 키를 설정에 붙여넣기만** → OS 키체인/자격 증명 저장소 |
| 하지 않음 | Claude 콘솔 자동 로그인·키 스크래핑, 스트리밍(MVP), 설치 패키지 배포 |

설치용 `.msi` / `.dmg` 등은 **아직 게시하지 않았습니다.** 지금은 소스에서 `tauri dev` / 로컬 빌드로 실행합니다.

> 계획(미구현): `.useupclaude/handoff.md` 핸드오프 포맷 — 아직 코드에 없습니다.

## 사전 준비

- **Node.js** 20+
- **Rust** stable (Tauri 2 → 1.85+ 권장)
- **[Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)** (`PATH` 또는 설정에서 경로 지정)
- **Anthropic API 키** (`sk-ant-…`) — 앱 설정 화면에만 붙여넣기
- OS별 WebView:
  - **Windows:** [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)
  - **macOS:** 기본 WebView (Xcode Command Line Tools)
  - Linux(개발용): WebKitGTK 등 Tauri 문서 참고

## 실행 방법

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

로컬 바이너리만 빌드할 때(인스톨러 없음):

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

첫 실행: **설정** → API 키 저장 → `claude` 경로 탐지 → **새 채팅**에서 프로젝트 폴더(cwd) 선택 후 메시지 전송.

## CLI와의 관계

```bash
# 첫 턴
claude -p "<prompt>" --output-format json --allowedTools Read

# 같은 세션 이어가기
claude -p "<prompt>" --resume "<session_id>" --output-format json --allowedTools Read
```

`ANTHROPIC_API_KEY`는 자식 프로세스 **환경변수로만** 주입합니다. argv·로그·이 저장소에 키를 넣지 마세요. `.env`에 키를 커밋하지 마세요.

## MVP 기본값

- 도구: 읽기 전용(`--allowedTools Read`)이 기본
- 응답: 완성 JSON을 받은 뒤 표시 (스트리밍 없음)
- 배포: 개인 사이드로드 / 로컬 도구

## 라이선스·기여

개인·실험용 MVP입니다. 이슈·PR 환영. 보안상 API 키·세션 덤프는 이슈에 올리지 마세요.
