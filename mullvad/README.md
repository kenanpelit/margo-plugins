# mullvad (margo)

**Mullvad VPN** control for the margo shell — a rich in-shell panel, ported from
the noctalia-shell plugin and built up. WASM plugin (`mplugin-sdk`).

- **Pill:** the active relay when connected (e.g. `de-ber-wg-006`), or `off`.
- **Panel** (layer-shell; size + position from the plugin's gear):
  - status hero — connected relay + visible location, or disconnected,
  - **Connect / Disconnect** + **Reconnect**,
  - **Lockdown mode** and **Auto-connect** toggles,
  - a **searchable country list** (relay counts) — click a country to connect.
- **CLI:** `mshellctl menu plugin mullvad` toggles the panel.

It drives the `mullvad` CLI via the host `run` capability (the same trust level
as a declarative plugin's shell commands — a sandboxed WASM panel that controls
a real system tool).

## Setup

1. Install + log in to **Mullvad** (`mullvad account login`).
2. Build mshell with `--features wasm-plugins` (the panel is WASM). Without it,
   the pill falls back to a `mullvad status` notification.
3. Settings → Plugins → install **Mullvad VPN**, enable it, place the pill.
4. Click the pill → the control panel. Tune size/position from the gear.

## Rebuilding `plugin.wasm`

```sh
cd mullvad
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/mullvad.wasm ./plugin.wasm
```
