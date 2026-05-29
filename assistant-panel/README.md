# assistant-panel (margo)

An AI assistant for the margo shell, using **Google Gemini**. Set your model and
API key in the plugin's settings; clicking the pill opens a **streaming chat**.

## Two ways it runs

- **In-shell panel** — on an mshell built with `--features wasm-plugins`, the
  pill opens a sandboxed WASM chat panel right in the shell: a scrollable log of
  message bubbles + an entry, with Gemini's reply streamed in token-by-token.
  This is `plugin.wasm` (built from `src/`, via the `mplugin-sdk`).
- **Terminal fallback** — on a standard mshell build, the same pill runs a
  multi-turn terminal chat (`chat.sh`, curl + jq — no extra CLI to install).

## Setup

1. Settings → Plugins → install/update **Assistant Panel**, open its **gear**,
   choose a **model** and paste your **Gemini API key**
   ([aistudio.google.com](https://aistudio.google.com/app/apikey)). Optionally
   set an **endpoint** (a proxy base URL; blank = Google).
2. Enable it and place **Assistant Panel · assistant** in a bar.
3. Click the pill:
   - wasm-plugins build → the in-shell panel (type, press Enter, watch it stream);
   - otherwise → the terminal chat (needs `curl` + `jq` and a terminal, default
     `kitty`).

## Rebuilding `plugin.wasm`

```sh
cd assistant-panel
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/assistant_panel.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

> The API key is stored in `~/.config/margo/mshell/plugins.toml` (mode `0600`).
> It's a local dotfile, not an OS keyring — don't commit it, and rotate it if it
> ever leaks.
