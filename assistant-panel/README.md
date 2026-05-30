# assistant-panel (margo)

A **multi-provider AI chat** for the margo shell. Pick a provider, set your
model + API key in the plugin's settings, and click the pill for a **streaming
chat panel** right in the shell.

Inspired by Dank Material Shell's `dms-ai-assistant`, ported to margo's WASM
plugin tier (`mplugin-sdk`) and styled with the shell's design language.

## Providers

| Provider     | Transport | Notes |
|--------------|-----------|-------|
| **gemini**   | SSE       | Google `streamGenerateContent`. Default. |
| **openai**   | SSE       | OpenAI `chat/completions`. Also covers LocalAI / LM Studio / vLLM / Inception — point `endpoint` at them. |
| **anthropic**| SSE       | Claude Messages API (`content_block_delta` deltas). |
| **ollama**   | NDJSON    | Local `/api/chat` with `stream: true`. No API key. |
| **custom**   | SSE       | Alias of `openai` for any OpenAI-compatible endpoint. |

Each provider has its own request shape and chunk parser; the active provider is
captured when you hit send, so switching providers or stopping mid-stream never
confuses the parser.

## Features

- **Streaming** — replies arrive token-by-token (via the host's `http-start`).
- **Markdown bubbles** — bold / italic / `code`, per-role label above each.
- **Persistent history** — saved to
  `$XDG_DATA_HOME/mshell/plugins/assistant-panel/session.json`, tagged with the
  provider (a provider switch drops the log instead of cross-feeding turns).
  Capped at 80 messages. Toggle off with the **Persist history** setting.
- **Header actions** —
  - **Stop** (while streaming) — drops the rest of the in-flight reply.
  - **Retry** — resends your last message.
  - **Copy** — puts the latest reply on the clipboard.
  - **New** — clears the conversation.
- **System prompt**, **temperature**, and **max tokens** are all configurable.
- **Inline errors** — a failed request (bad key, quota, wrong model) surfaces the
  provider's error message above the input instead of a silent empty bubble.

## Two ways it runs

- **In-shell panel** — on an mshell built with `--features wasm-plugins`, the
  pill opens the sandboxed WASM chat panel (`plugin.wasm`, built from `src/`).
- **Terminal fallback** — on a standard mshell build, the same pill runs a
  multi-turn terminal chat (`chat.sh`, curl + jq, Gemini-only — no extra CLI to
  install).

## Setup

1. Settings → Plugins → install/update **Assistant Panel**, open its **gear**:
   - **Provider** — gemini · openai · anthropic · ollama · custom.
   - **Model** — provider-specific id, e.g. `gemini-2.5-flash`, `gpt-4o-mini`,
     `claude-sonnet-4-5-20250929`, `llama3`, `qwen2.5`.
   - **API Key** — your provider key (skip for Ollama / local custom endpoints).
   - **Endpoint override** — optional base URL (proxy, LocalAI, LM Studio, vLLM).
     Blank = the provider default.
   - **Temperature** / **Max tokens** / **System prompt** — optional tuning.
   - **Persist history** — yes / no.
2. Enable it and place **Assistant Panel · assistant** in a bar
   (or use the suggested **Super+A** hotkey).
3. Click the pill (or press the hotkey):
   - wasm-plugins build → the in-shell panel (type, press Enter, watch it stream);
   - otherwise → the terminal chat (needs `curl` + `jq` and a terminal, default
     `kitty`).

### API key sources

- **OpenAI** — <https://platform.openai.com/api-keys>
- **Anthropic** — <https://console.anthropic.com/settings/keys>
- **Gemini** — <https://aistudio.google.com/app/apikey>
- **Ollama** — none; run `ollama serve` locally.

## Rebuilding `plugin.wasm`

```sh
cd assistant-panel
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/assistant_panel.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

> The API key is a `secret` setting, stored in the OS **keyring** (the
> secret-service backend), not in a plaintext dotfile. Rotate it if it ever
> leaks.
