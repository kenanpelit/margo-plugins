# ambient-sound (margo)

Ambient soundscapes for focus & sleep — an in-shell **sound mixer** for the
margo shell. Mix any of 14 looping sounds, save your favourite combinations as
presets, and let a **sleep timer** stop everything (or lock / suspend) when
you drift off.

A port of [`dms-ambient-sound`](https://github.com/hthienloc/dms-ambient-sound)
to margo's WASM plugin tier (`mplugin-sdk`), restyled with the shell's design
language (`mshell-frame/DESIGN.md`).

## Sounds

Rain · Storm · Wind · Waves · Stream · Birds · Summer Night · Fireplace ·
Coffee Shop · City · Train · Boat · White Noise · Pink Noise

Each plays as its own looping `mpv` instance, so you can layer as many as you
like (e.g. **Rain + Birds + Wind**).

## Features

- **Mix & match** — toggle any number of sounds; they loop independently.
- **Master volume + mute** — pushed live to every playing sound over mpv's
  JSON IPC socket.
- **Presets** — save the current mix, load it back with one click, delete the
  ones you don't want. Ships with a "Relaxing Rain" default.
- **Sleep timer** — 15m / 30m / 45m / 1h / 1.5h / 2h, with a **When done**
  action: **Stop**, **Lock screen**, or **Suspend**. The countdown runs as a
  detached background process, so it fires even after the panel is closed.

## How it works

The panel never blocks the UI on audio: it shells out to a bundled helper
(`ambient.sh`) through the host's `run` capability, and the helper backgrounds
one detached `mpv` per sound. State (volume, presets, timer action) is
persisted to the plugin's data dir and reconciled with the live `mpv`
processes whenever the panel reopens — so your soundscape keeps playing while
the panel is closed.

## Requirements

- **mpv** — audio playback
- **socat** — live volume control over mpv's IPC socket
- **setsid** (util-linux) + **pkill/pgrep** (procps) — detach & manage the
  player processes
- An mshell built with `--features wasm-plugins` for the in-shell panel.

## Setup

1. Settings → Plugins → install/enable **Ambient Sound**.
2. Place **Ambient Sound · ambient** in a bar (or use the suggested
   **Super+Shift+A** hotkey).
3. Click the pill → the mixer opens. Tap a sound to start it; adjust the
   master volume with −/+; save a mix as a preset; set a sleep timer.

## Rebuilding `plugin.wasm`

```sh
cd ambient-sound
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/ambient_sound.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

Sounds and the original concept are from
[`dms-ambient-sound`](https://github.com/hthienloc/dms-ambient-sound) by
Loc Huynh (GPL-3.0).
