# clipper (margo)

A clipboard manager for the margo shell — searchable history, one-click copy,
per-entry delete, clear-all, and **pinned items** that survive history
rotation. A best-of-both port of noctalia's `clipper` and DMS's `ClipboardPlus`,
built on the same backend they use: [`cliphist`](https://github.com/sentriz/cliphist)
+ wl-clipboard.

Three tabs: **Clipboard · Pinned · Notes**.

## Features

### Clipboard
- **History** — every clip cliphist has recorded; **image entries show a real
  thumbnail** (decoded once and cached), not just a label.
- **Search** — type in the box and press Enter to filter the whole history.
- **Copy** — click any entry to put it back on the clipboard (`cliphist decode
  | wl-copy`, works for text *and* images).
- **Delete / Clear all** — 🗑 one entry, or wipe the whole history.
- **Auto-paste** (optional) — after copying, replay Ctrl+V into the focused
  window (`wtype`); off by default.

### Pinned
- **☆ Pin** a text clip → its full content is stored in the plugin data dir,
  so it survives even after the clip rotates out of cliphist. Click to copy.

### Notes (sticky notes)
- **Create / edit / delete** colour-coded sticky notes, persisted in the plugin
  data dir.
- **Recolour** — cycle a 5-colour palette (yellow / pink / blue / green /
  purple); the colour shows as a swatch on each card.
- **Copy** a note's text with one click.

## Requirements

- **cliphist** — the clipboard history daemon. Make sure it's running, e.g. in
  your compositor autostart:
  ```
  wl-paste --type text --watch cliphist store
  wl-paste --type image --watch cliphist store
  ```
- **wl-clipboard** (`wl-copy`) — to put entries back on the clipboard.
- **wtype** — only if you enable auto-paste.

## Setup

1. Settings → Plugins → install/enable **Clipper**.
2. Place **Clipper · clipboard** in a bar (or use **Super+V**).
3. Click the pill → search, click to copy, ☆ to pin.

### Settings

- **History items shown** — how many recent clips to list (search still scans
  the full history). Default 60.
- **Auto-paste on select** — simulate Ctrl+V after copying (needs wtype).

## Rebuilding `plugin.wasm`

```sh
cd clipper
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/clipper.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

Concept + feature set from
[noctalia clipper](https://github.com/noctalia-dev/noctalia-plugins) by
blackbartblues and [ClipboardPlus](https://github.com/Dadangdut33) by
Dadangdut33. Backend: cliphist by sentriz.
