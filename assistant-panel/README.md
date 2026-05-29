# assistant-panel (margo)

An AI assistant for the margo shell. **This is the declarative port** of the
concept: a bar pill + a settings form (provider / model / **API key** /
terminal) whose values flow into the launch command. Clicking the pill opens
an interactive chat in your terminal.

## What's ported vs. not

margo is a compiled shell, so — unlike the QML original — it can't host an
arbitrary in-shell chat panel from plugin data. So this port covers the parts
the **declarative** plugin tier supports:

- ✅ bar pill
- ✅ settings: provider, model, **API key** (`secret`), terminal
- ✅ launches a chat (external terminal CLI) using those settings
- ⏳ the **in-shell chat panel** (bubbles, streaming input) — needs the
  upcoming **WASM plugin tier**; until then the chat runs in a terminal.

## Setup

1. Install [`aichat`](https://github.com/sigoden/aichat) (or change the
   `on_click` command to your preferred AI CLI).
2. Settings → Plugins → install **Assistant Panel**, open its **gear**, set
   your provider / model / API key.
3. Enable it and place **Assistant Panel · assistant** in a bar.
4. Click the pill (or its **Chat** menu row) to start chatting.

> The API key is stored in `~/.config/margo/mshell/plugins.toml` (mode `0600`).
> It's a local dotfile, not an OS keyring — don't commit it.

The default command targets OpenAI (`OPENAI_API_KEY`); for other providers,
adjust the `on_click` env var / model string in the manifest.
