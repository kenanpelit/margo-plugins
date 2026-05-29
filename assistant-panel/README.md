# assistant-panel (margo)

An AI assistant for the margo shell, using **Google Gemini**. Set your model
and API key in the plugin's settings; clicking the pill opens a multi-turn chat
in your terminal (bundled `chat.sh`, via curl + jq — no extra CLI to install).

## What's ported vs. not

margo is a compiled shell, so — unlike the QML original — it can't yet host an
arbitrary in-shell chat panel from plugin data. So this covers the declarative
parts; the chat runs in a terminal for now:

- ✅ bar pill + settings: **model** (choice), **API key** (`secret`), terminal
- ✅ a working multi-turn Gemini chat (`chat.sh`, curl + jq), launched with your
  settings substituted in (incl. `{{plugin_dir}}/chat.sh`)
- ⏳ the **in-shell chat panel** (bubbles, streaming input) — the WASM plugin
  tier (in progress); until then the chat is a terminal window.

## Setup

1. Have `curl` + `jq` installed (both are standard), and a terminal (default
   `kitty`).
2. Settings → Plugins → install/update **Assistant Panel**, open its **gear**,
   choose a **model** and paste your **Gemini API key**
   ([aistudio.google.com](https://aistudio.google.com/app/apikey)).
3. Enable it and place **Assistant Panel · assistant** in a bar.
4. Click the pill (or its **Chat** menu row) to start chatting.

> The API key is stored in `~/.config/margo/mshell/plugins.toml` (mode `0600`).
> It's a local dotfile, not an OS keyring — don't commit it, and rotate it if
> it ever leaks.
