# mullvad (margo)

Control **Mullvad VPN** from the margo bar — ported from the noctalia-shell
plugin. A status pill plus a layer-shell control menu.

- **Pill:** shows the active relay when connected (e.g. `de-ber-wg-006`), or
  `off` when disconnected. Polled every 5s.
- **Menu** (opens as a first-class layer-shell menu): **Connect**,
  **Disconnect**, **Reconnect**, **Lockdown: On/Off**, **Show status**.
- **Size & position:** set in this plugin's **gear** (Settings → Plugins →
  Mullvad VPN) — same controls as any panel/menu plugin.

This is a declarative plugin (no compiled code): it drives the `mullvad` CLI
via shell commands, which is exactly what VPN control needs. (A sandboxed WASM
plugin can't run processes by design, so the CLI control lives in the
declarative tier.)

## Setup

1. Install + connect the **Mullvad** app / CLI (`mullvad account login`, etc.).
2. Settings → Plugins → install **Mullvad VPN**, enable it, place
   **Mullvad VPN · vpn** in a bar.
3. Click the pill for the control menu. Tune its size/position from the gear.
