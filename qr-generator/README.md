# qr-generator (margo)

Generate a QR code from text, the clipboard, or your current Wi-Fi network —
shown right in the panel, copyable as text or PNG. A port of Dank Material
Shell's `dms-qr-generator` to margo's WASM plugin tier (`mplugin-sdk`).

## Features

- **From text** — type a string or URL, press Enter (or the button) → QR.
- **From clipboard** — one click encodes whatever's on the clipboard.
- **Wi-Fi share** — builds a `WIFI:` payload from your active network (SSID +
  password via `nmcli`) so a phone can join by scanning.
- **Copy** — copy the encoded text, or the **QR PNG itself** to the clipboard
  (`wl-copy -t image/png`) to paste into chats/docs.

## Requirements

- **qrencode** — QR rendering (`sudo pacman -S qrencode`).
- **wl-clipboard** — for "Copy image" and "From clipboard".
- **nmcli** (NetworkManager) — only for the Wi-Fi button.

## Setup

1. Settings → Plugins → install/enable **QR Generator**.
2. Place **QR Generator · qr** in a bar (or use **Super+Shift+Q**).
3. Click the pill → type / paste / Wi-Fi → the QR appears.

### Settings

- **QR pixel scale** — qrencode module size (`-s`); bigger = larger image.

## How it works

The text is handed to `qrencode` as a single argv element after `--`, so there
is no shell quoting and arbitrary content (URLs, Wi-Fi payloads with `;`/`:`)
is safe. The PNG is written under `/tmp/margo-qr/` and shown via an `image`
node; each generation uses a fresh filename so the preview always refreshes.

## Rebuilding `plugin.wasm`

```sh
cd qr-generator
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/qr_generator.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).

## Credits

Concept from [`dms-qr-generator`](https://github.com/hthienloc/dms-qr-generator)
by Loc Huynh. QR rendering by qrencode.
