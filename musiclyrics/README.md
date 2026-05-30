# musiclyrics (margo)

Time-synced lyrics for whatever's playing — an in-shell panel for the margo
shell that reads the current MPRIS track, fetches synced lyrics from
[lrclib.net](https://lrclib.net), and **highlights the current line live** as
the song plays. Inspired by Dank Material Shell's `dms-plugin-musiclyrics`,
ported to margo's WASM plugin tier (`mplugin-sdk`).

## Features

- **Live synced highlight** — the active line steps forward (larger, primary
  colour) and advances with the song; the panel windows the lines around it so
  the current one is always in view.
- **lrclib.net source** — free, no account, no API key. Matched by artist +
  title + album + duration for accuracy.
- **Local cache** — fetched lyrics are saved under the plugin's data dir, so
  the same track loads instantly next time (toggleable).
- **Plain-lyrics fallback** — if a track has no synced lyrics but has plain
  ones, they're shown unsynced.
- **Bar pill line** — the pill can mirror the current line (active once the
  panel has been opened in the session).
- **Sync offset** — nudge the highlight earlier/later if your player reports
  position slightly off.

## How it works

The WASM tier has no timer, so the panel runs a tiny 1 Hz heartbeat subprocess
(`process-start`) that re-reads the MPRIS position each second and moves the
highlight. Lyrics are fetched asynchronously over the host's `http-start` (the
network request never blocks the UI). No auth, no shell-outs beyond the
heartbeat.

## Setup

1. Settings → Plugins → install/enable **Music Lyrics**.
2. Place **Music Lyrics · lyrics** in a bar (or use **Super+Shift+L**).
3. Play something (any MPRIS player) and open the panel — lyrics load and track
   the song. Use the ⟳ button to force a re-fetch.

### Settings

- **Sync offset (ms)** — shift the highlighted line (negative = earlier).
- **Cache lyrics** — save fetched lyrics locally (default yes).
- **Show line in bar pill** — write the current line to a file the pill reads.

## Rebuilding `plugin.wasm`

```sh
cd musiclyrics
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/musiclyrics.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

Concept from [`dms-plugin-musiclyrics`](https://github.com/Gasiyu/dms-plugin-musiclyrics)
by gasiyu. Lyrics from [lrclib.net](https://lrclib.net).
