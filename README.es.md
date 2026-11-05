[한국어](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [Español](README.es.md)

# UseUpClaude

Chat de escritorio alrededor de `-p` / `--resume` de Claude Code CLI. Cada mensaje llama una vez a `claude -p … --output-format json`. La app muestra el texto `result` como markdown, no el sobre JSON. `session_id` se queda dentro de la app para `--resume`.

> **claude -p tiene menos contexto que Claude Code interactivo y puede no completar el trabajo igual de bien.**

El título de la ventana y el nombre del producto son UseUpClaude. El ejecutable sigue llamándose `claude-gui`. El crate de Rust no cambió.

## Qué hace

| Parte | Comportamiento |
|------|----------------|
| Chat | Burbujas, avatares, Enter para enviar, estado de trabajo. Sin streaming de tokens |
| Respuesta | `result` del JSON del CLI, renderizado como markdown GFM |
| Coste | `total_cost_usd` de ese turno cuando el CLI lo informa |
| Saldo | Una clave de API normal no puede leer el crédito restante. No se raspa la consola |
| Proyectos | Chats agrupados por carpeta de trabajo (cwd) |
| Sesiones existentes | Lee el tip JSONL en `~/.claude/projects`, importa texto de usuario/asistente para mostrarlo y usa `--resume` desde el tip más reciente en cada envío |
| Idiomas | 한국어, English, 日本語, 简体中文, Español. Locale del sistema; si no, coreano |
| Auth | Pega una clave de Anthropic en Ajustes. Queda en el almacén del sistema y no se registra |

## Requisitos

- Node.js 20+
- Rust stable (Tauri 2, 1.85+ recomendado)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
- Una clave de API de Anthropic (`sk-ant-…`)
- Windows: [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)

## Ejecutar

```bash
git clone https://github.com/bheejun/UseUpClaude.git
cd UseUpClaude
npm install
npm run tauri dev
```

Si ya hay una build de depuración, `npm run tauri dev` compila de forma incremental. Binario de release sin instalador:

```bash
npm run tauri build -- --no-bundle
./src-tauri/target/release/claude-gui
```

En Windows: `src-tauri\target\release\claude-gui.exe`.

Primera vez: **Ajustes** → guardar la clave → detectar `claude` → **Nuevo chat** y elegir la carpeta.

`ANTHROPIC_API_KEY` solo se pasa como entorno del proceso hijo. No pongas la clave en argv, registros ni este repositorio.
