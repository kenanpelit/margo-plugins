# breathing (margo)

Guided breathing exercises for calm & focus — an in-shell panel for the margo
shell with a live **breathing orb** that expands on the inhale and contracts on
the exhale, phase coaching, and a session timer.

A port of [`dms-breathing`](https://github.com/hthienloc/dms-breathing) to
margo's WASM plugin tier (`mplugin-sdk`), restyled to the shell's design
language (`mshell-frame/DESIGN.md`).

## Techniques

| Technique | Pattern (in · hold · out · rest) |
|-----------|----------------------------------|
| Deep Breathing | 4 · 4 · 4 |
| 4-7-8 Breathing | 4 · 7 · 8 |
| Box Breathing | 4 · 4 · 4 · 4 |
| Equal Breathing | 4 · 4 |
| Resonance | 5.5 · 5.5 |
| Alternate Nostril | 4 · 4 · 2 |

## Features

- **Breathing orb** — a `breath-orb` visual that grows/shrinks with the breath,
  tinted per phase (primary inhale · warning hold · success exhale · neutral
  rest) straight from the matugen palette.
- **Phase coaching** — a big "Breathe In / Hold / Breathe Out / Rest" word plus
  a per-phase countdown.
- **Session timer** — 1 / 3 / 5 / 10 / 15 / 30 minutes, with the time remaining
  shown in the header; a notification (and optional chime) marks completion.
- **Pause / Resume / End** — stop any time; pausing freezes the breath.
- **Sound cues** (optional, off by default) — a soft chime at the start of each
  inhale; needs `paplay`.

## How it works

The WASM tier has no timer, so the panel starts a tiny detached ticker
subprocess (via the host's `process-start`) that emits a heartbeat every
100 ms. Each heartbeat advances the breathing clock — so the rhythm is paced by
ticks, not wall-clock, and stays steady. The ticker is killed cleanly on
stop/finish, so nothing lingers in the background.

## Requirements

- An mshell built with `--features wasm-plugins` for the in-shell panel.
- `paplay` (libpulse / pipewire-pulse) — only if you enable sound cues.

## Setup

1. Settings → Plugins → install/enable **Breathing**. Optionally set a default
   duration and turn on sound cues.
2. Place **Breathing · breathing** in a bar (or use **Super+Shift+B**).
3. Click the pill → pick a technique + duration → **Start session**.

## Rebuilding `plugin.wasm`

```sh
cd breathing
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/breathing.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

The chime sound and original concept are from
[`dms-breathing`](https://github.com/hthienloc/dms-breathing) by Loc Huynh
(GPL-3.0).
