# mplayerplus (margo)

A full media-player panel for the margo shell — album art, a live progress
bar, and real transport controls for whatever's playing over MPRIS. Inspired
by Dank Material Shell's MediaControlPlus, ported to margo's WASM plugin tier
(`mplugin-sdk`) and focused on *controls* rather than a visualizer.

## Features

- **Album art** — the track's artwork, rounded, as the focal card (falls back
  to a music glyph when none is available).
- **Live progress** — a slim progress bar with elapsed / total time, advanced
  by a 1 Hz heartbeat so it tracks the song.
- **Transport** — play/pause (round primary button), previous / next, and
  ±N-second seek.
- **Shuffle & repeat** — toggles that light up when active; repeat cycles
  None → Playlist → Track.
- **Targets the right player** — controls the player the shell is showing (via
  its MPRIS name), so it stays in sync with the bar's media widget.

## How it works

State (title / artist / album / art / position / length / player) comes from
the host's `media-now-playing` capability. Control is delegated to
[`playerctl`](https://github.com/altdesktop/playerctl) through the host's `run`
capability, scoped to the active player with `playerctl -p <name> …`. A 1 Hz
heartbeat subprocess keeps the progress bar and shuffle/repeat state fresh.

## Requirements

- **playerctl** — transport control (`pacman -S playerctl`).
- An mshell built with `--features wasm-plugins` for the in-shell panel, recent
  enough that `media-now-playing` reports the live position + player name.

## Setup

1. Settings → Plugins → install/enable **Media Player Plus**.
2. Place **Media Player Plus · player** in a bar (or use **Super+Shift+M**).
3. Play something and open the panel.

### Settings

- **Seek step (seconds)** — how far ⏪ / ⏩ jump (default 10).
- **Show album art** — toggle the artwork (default yes).

## Rebuilding `plugin.wasm`

```sh
cd mplayerplus
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/mplayerplus.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

Concept from [MediaControlPlus](https://github.com/Dadangdut33) by Dadangdut33.
