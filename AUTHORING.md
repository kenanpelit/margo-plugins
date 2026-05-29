# Writing margo plugins

This guide is for plugin **authors** — whether you're contributing to this
repo or publishing your own source. For installing plugins, see the
[README](README.md); for where the system is headed, see the
[roadmap](road_map.md).

## What a plugin is (and isn't)

mshell is a compiled Rust/GTK shell, so — unlike QML-based shells — it can't
load arbitrary downloaded UI code. A margo plugin is therefore **declarative**:
it ships no compiled code. It's a `manifest.toml` (plus optional asset files)
that describes one or more **bar widgets**, which mshell renders with its
built-in custom-widget engine.

A plugin widget can:

- show an icon and/or a label,
- fill that label from a shell command's output, refreshed on an interval,
- run a command on left- and right-click,
- carry a tooltip.

A plugin widget **cannot** (yet) draw arbitrary UI, add menus/popups, or
contribute settings pages. That's the trade-off for a compiled shell — richer
plugin tiers (scripting / sandboxed) are on the [roadmap](road_map.md).

## Quick start: your own source

A *source* is just a git repo with a `registry.toml` at its root. To make one:

```
my-margo-plugins/
├── registry.toml          # lists the plugins this repo offers
└── hello/
    └── manifest.toml       # one plugin
```

`registry.toml`:

```toml
[[plugins]]
id = "hello"
dir = "hello"
version = "1.0.0"
name = "Hello"
min_mshell = "0.8.8"
description = "A hello-world pill"
```

`hello/manifest.toml`:

```toml
id = "hello"
name = "Hello"
version = "1.0.0"
min_mshell = "0.8.8"

[[widget]]
key = "greeting"
icon = "face-smile-symbolic"
label = "hello"
on_click = "notify-send 'hi from a plugin'"
```

Push it to a public git repo, then in mshell open **Settings → Plugins →
Sources**, add the repo URL, hit **Refresh**, and install.

## Repository layout

```
<source-repo>/
├── registry.toml           # index — fetched first (shallow), lists every plugin
├── <plugin-id>/
│   ├── manifest.toml        # plugin metadata + widget definitions
│   └── assets/              # optional images referenced by the manifest
└── …
```

mshell never clones the whole repo: it sparse-clones just `registry.toml` to
list plugins, then sparse-clones a single plugin's folder when you install it.

## `registry.toml`

One `[[plugins]]` table per plugin. `id`, `dir`, and `version` are required.

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Unique within this source. No `:` or `/`. Must match the manifest `id`. |
| `dir` | yes | Folder in the repo holding this plugin's `manifest.toml`. |
| `version` | yes | `x.y.z`. Should match the manifest `version`. |
| `name` | no | Display name in the Available list. |
| `min_mshell` | no | Minimum mshell version (`x.y.z`). |
| `description` | no | One-line blurb shown in Available. |

## `manifest.toml`

Plugin-level fields, then one or more `[[widget]]` tables.

| Field | Required | Default | Meaning |
|---|---|---|---|
| `id` | yes | — | Stable id, unique within the source. No `:` or `/`. |
| `name` | no | `""` | Human-readable name. |
| `version` | yes | — | `x.y.z`. |
| `author` | no | `""` | Your handle. |
| `min_mshell` | no | `""` (no floor) | Minimum mshell version. |
| `description` | no | `""` | Short description. |

### `[[widget]]`

Each widget becomes a placeable bar pill. Fields mirror mshell's custom-widget
vocabulary:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `key` | string | — (**required**) | Unique within the plugin. Placed in a bar as `plugin:<key>:<this>` (see Naming). |
| `icon` | string | `""` | Symbolic icon name from the active icon theme (MargoMaterial → Adwaita), e.g. `network-transmit-receive-symbolic`. |
| `image` | string | `""` | Image file path **relative to the plugin folder**. Takes precedence over `icon`. |
| `label` | string | `""` | Static label. Ignored when `exec` is set. |
| `tooltip` | string | `""` | Hover tooltip. |
| `exec` | string | `""` | Command run via `sh -c`; its stdout becomes the label. |
| `template` | string | `""` | Label template; `{output}` = trimmed `exec` stdout. Empty = use stdout verbatim. |
| `interval` | integer (secs) | `0` | How often to re-run `exec`. `0` = run once. |
| `on_click` | string | `""` | Command run via `sh -c` on left-click. |
| `on_click_right` | string | `""` | Command run via `sh -c` on right-click. |
| `max_chars` | integer | `0` | Truncate the rendered label to N chars. `0` = no cap. |

## The widget model in depth

**Label sources.** If `exec` is set, the label is whatever the command
prints (run through `template`, truncated to `max_chars`). Otherwise the
static `label` is shown. The widget hides its label area when empty.

**Polling.** With `exec` and `interval = N`, mshell re-runs the command every
N seconds and updates the label. `interval = 0` runs it exactly once. The
command runs asynchronously — a slow command (or a `sleep` inside it) never
blocks the shell.

**Computing rates / deltas.** A single `exec` run can't diff two points in
time, so do both reads inside one command with a `sleep` between them. This is
how the bundled `network-speed` plugin works:

```sh
a=$(read_counters); sleep 1; b=$(read_counters); compute_delta "$a" "$b"
```

**Inline vs. scripts.** `exec` runs via `sh -c` with **no guaranteed working
directory**, so referencing a script file by relative path won't work
reliably. Keep the command **inline** in the manifest (a one-liner, or a
`'''…'''` TOML multi-line literal), or call an absolute path / a program
already on `$PATH`.

**Icons.** `icon` is looked up in the active GTK icon theme. mshell ships the
**MargoMaterial** symbolic set (falling back to Adwaita), so any
`*-symbolic` name from those themes works. For a bespoke glyph, ship an SVG/PNG
in your plugin's `assets/` and point `image` at it (relative path).

**Clicks.** `on_click` / `on_click_right` are fire-and-forget `sh -c`
commands. The shell env is the systemd user session — it does **not** inherit
your interactive shell rc, so set any needed env vars explicitly in the
command.

## Naming & placement

- **Plugin `id`**: lowercase, no `:` or `/`. Unique within your source.
- **Widget `key`**: unique within the plugin.
- When enabled, each widget is registered as a custom widget named
  `plugin:<key>:<widget-key>`, where `<key>` is the plugin's *composite key*:
  - plugins from the **official** source keep the plain `id` →
    `plugin:network-speed:rate`;
  - plugins from a **custom** source get a short source-hash prefix →
    `plugin:ab12cd:network-speed:rate`, so two sources can ship the same `id`
    without colliding.
- To place it: **Settings → Bar → add widget** lists every enabled plugin
  widget by that name.

## Versioning

Bump `version` (in both `manifest.toml` and the `registry.toml` entry) on every
change. Set `min_mshell` if your widget needs behaviour from a specific mshell
release; it's recorded so the manager can warn / hold back the plugin on older
shells (compatibility gating is still being wired up).

## Security & trust

Plugins run shell commands (`exec`, `on_click`, `on_click_right`) **with the
user's privileges** — there is no sandbox. Treat installing a plugin as
trusting it to run code.

- Installing only downloads files; it never runs anything.
- The **Installed** list shows every command a plugin declares so users can
  review them.
- Code runs only once the user **enables** the plugin — that's the trust gate.

Author accordingly: keep commands transparent, avoid surprising side effects,
and don't fetch-and-execute remote code.

## Validation rules

A manifest is rejected on install unless:

- `id` is non-empty and contains no `:` or `/`;
- `version` is non-empty;
- every `[[widget]]` has a non-empty `key`;
- widget keys are unique within the plugin.

## Testing your plugin

1. **Run the command by hand** exactly as written:
   ```sh
   sh -c '<your exec command>'
   ```
   Confirm it prints what you expect, quickly, with no prompts.
2. **Validate the TOML** parses (any TOML linter, or `python3 -c "import
   tomllib,sys; tomllib.load(open(sys.argv[1],'rb'))" manifest.toml`).
3. **Install from your source** in Settings → Plugins, enable it, and place it
   from Settings → Bar.

## Publishing

- **Your own source:** push your repo, share the URL — users add it under
  Settings → Plugins → Sources. Nothing else is required.
- **The official source** (this repo): add a `<plugin-id>/` folder + a
  `registry.toml` entry and open a pull request.

## A worked example: `network-speed`

The bundled [`network-speed`](network-speed/manifest.toml) plugin is a
complete reference — a single polling widget that samples `/proc/net/dev`
twice a second apart and prints `↓ rate  ↑ rate`. Read its `manifest.toml`
to see the inline-`exec`, `template`, and `interval` fields in practice.

## Limitations & what's next

Declarative widgets cover the common status / info / action pills. Arbitrary
UI, popup menus, and per-plugin settings need the scripting (Lua) or sandboxed
(WASM) tiers — see the [roadmap](road_map.md).
