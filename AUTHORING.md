# Authoring margo plugins

A complete guide for writing plugins for the **margo** desktop shell
(mshell). Whether you want to add a tiny bar pill or ship a full in-shell
control panel, this document has everything you need.

For installing existing plugins see the [README](README.md); for where the
plugin system is headed see the [roadmap](road_map.md).

---

## Contents

- [1. What a margo plugin is](#1-what-a-margo-plugin-is)
- [2. The two tiers](#2-the-two-tiers)
- [3. Quick start — your first plugin in five minutes](#3-quick-start--your-first-plugin-in-five-minutes)
- [4. Anatomy of a plugin](#4-anatomy-of-a-plugin)
- [5. The manifest](#5-the-manifest)
- [6. Declarative tier — pills, polls, menus](#6-declarative-tier--pills-polls-menus)
- [7. WASM tier — sandboxed panels](#7-wasm-tier--sandboxed-panels)
- [8. The SDK in depth](#8-the-sdk-in-depth)
- [9. Host capabilities](#9-host-capabilities)
- [10. Settings & secrets](#10-settings--secrets)
- [11. State & persistence](#11-state--persistence)
- [12. The development loop](#12-the-development-loop)
- [13. The design language](#13-the-design-language)
- [14. Publishing](#14-publishing)
- [15. Worked examples](#15-worked-examples)
- [16. Troubleshooting](#16-troubleshooting)
- [Appendix A — full host WIT](#appendix-a--full-host-wit)
- [Appendix B — SDK API](#appendix-b--sdk-api)

---

## 1. What a margo plugin is

A margo plugin is a small piece of **content** that mshell loads from your
git repository at runtime. It can add one or more **bar widgets** (the pills
in mshell's top/bottom bars), **menus**, or **full panels** that open over
the shell.

The two design pillars:

- **Capability-based sandboxing.** Unlike QML/JavaScript shells (Quickshell,
  Noctalia, AGS, …) where a plugin gets the whole world, a margo plugin is
  either a string in TOML or a wasm32 component that can only call the
  capabilities the host declares (`http`, `notify`, `read-file`, …). The
  sandbox is the WIT contract in [`wit/world.wit`](https://github.com/kenanpelit/margo/blob/main/mshell-crates/mshell-plugin-host/wit/world.wit).
- **Language-agnostic.** Anything that compiles to `wasm32-wasip2` works —
  Rust, Go, AssemblyScript, C, Zig. The official `mplugin-sdk` is Rust, but
  the host doesn't care.

A plugin **never** writes native code linked into mshell, never gets a
`dlopen` handle, never sees other plugins' data. It gets a flat node-list
protocol and a small handful of host calls.

---

## 2. The two tiers

You can ship a plugin at one of two levels of richness:

| Tier | Build artefact | What you can do | When to use |
|---|---|---|---|
| **Declarative** | A `manifest.toml`, nothing more | A bar pill with an icon, a label fed by a shell command, click actions, a drop-down menu of more commands | "Show ↑/↓ network speed", "Click → show `ufw status`" — anything a shell one-liner can express |
| **WASM** | `manifest.toml` + a compiled `plugin.wasm` (~120 KB) | A full in-shell panel with text, sliders, switches, lists, streaming chat, scoped fs, subprocess streams | Anything with live state — chat, VPN control, a todo list, a music remote |

Both tiers share the same `manifest.toml` skeleton and registry workflow.
You can mix them — a `[[widget]]` can be a declarative pill that opens a
declarative menu OR, on a `wasm-plugins` build of mshell, opens a WASM
panel. The pill's command is the fallback when the WASM panel can't render
(older mshell builds, missing component, …).

---

## 3. Quick start — your first plugin in five minutes

### Prerequisites

```sh
# A nightly-fresh stable Rust will do.
rustup target add wasm32-wasip2
# One-time: the cookiecutter.
cargo install cargo-generate
```

### Scaffold

```sh
cargo generate \
  --git https://github.com/kenanpelit/margo-plugins \
  --branch main \
  --name my-plugin \
  template
```

You get:

```
my-plugin/
├── Cargo.toml          # already wired to mplugin-sdk
├── manifest.toml       # declares the bar pill + opens the panel
└── src/lib.rs          # a working counter plugin
```

### Build + install

```sh
cd my-plugin
cargo build --target wasm32-wasip2 --release
mkdir -p ~/.config/margo/mshell/plugins/my-plugin
cp manifest.toml ~/.config/margo/mshell/plugins/my-plugin/
cp target/wasm32-wasip2/release/my_plugin.wasm \
   ~/.config/margo/mshell/plugins/my-plugin/plugin.wasm
```

### Enable + see it

Open Settings → Plugins. Your plugin shows up under **Installed**; toggle
it on, then drop a bar widget for `plugin:my-plugin:main` into a slot.
Click the new pill — the panel pops up.

That's it. From here, edit `src/lib.rs`, rebuild, copy the wasm — mshell's
file watcher hot-reloads the panel within a second.

---

## 4. Anatomy of a plugin

Every plugin lives in its own folder:

```
my-plugin/
├── manifest.toml      # required — declares everything the shell needs to know
├── plugin.wasm        # WASM tier only — the compiled component
├── icon.svg           # optional — referenced from manifest's icon field
├── src/lib.rs         # author source (not shipped to users)
├── Cargo.toml         # author build config (not shipped to users)
└── README.md          # optional — surfaced in Settings → Plugins eventually
```

When you publish through the registry, mshell clones the **entire folder**
into `~/.config/margo/mshell/plugins/<key>/`. Anything you put there is
visible to the plugin via the `plugin_dir` substitution (declarative tier)
or simply via relative paths (WASM tier).

The plugin's **composite key** depends on the source:

- Official source (`github.com/kenanpelit/margo-plugins`): key = plugin
  `id`. So this repo's `mullvad` is just `mullvad`.
- Third-party source: key = `<short-hash>:<id>`. Prevents id collisions
  between sources you've added.

---

## 5. The manifest

The `manifest.toml` is the only file the host **must** read. Full reference:

```toml
# ── Required ─────────────────────────────────────────────────────────────
id          = "my-plugin"        # kebab-case, no slashes or colons
name        = "My Plugin"        # human-readable
version     = "0.1.0"            # semver
author      = "your-name"

# ── Optional metadata ────────────────────────────────────────────────────
min_mshell  = "0.8.8"            # gate; the shell refuses older versions
description = "What this plugin does, one paragraph."
icon        = "applications-engineering-symbolic"   # gallery icon
preview     = ""                                     # screenshot URL (reserved)

# ── WASM tier (omit for declarative-only plugins) ────────────────────────
entry       = "plugin.wasm"      # the component shipped beside the manifest
entry_kind  = "wasm"             # the only value today

# ── One bar widget (you can repeat [[widget]] for multiple pills) ────────
[[widget]]
key          = "main"            # part of `plugin:<id>:<key>` — keep stable
icon         = "weather-clear-symbolic"
tooltip      = "What clicking does"
exec         = "echo hello"      # shell command; its stdout fills `{output}`
template     = "{output}"        # how to format the pill label
interval     = 30                # seconds; 0 = run once at start
max_chars    = 12                # truncate the label
opens_panel  = true              # only meaningful with a WASM `entry`
on_click     = "notify-send 'Hi'"        # left-click; ignored if opens_panel
on_click_right = "xdg-open https://…"    # right-click

  # Declarative drop-down menu (skipped if opens_panel + WASM panel renders)
  [[widget.menu]]
  label    = "Refresh"
  icon     = "view-refresh-symbolic"
  exec     = "systemctl --user restart my-thing"
  severity = "normal"             # "normal" | "warn" | "danger" — colours the row

  [[widget.menu]]
  label    = "Stop service"
  icon     = "process-stop-symbolic"
  exec     = "systemctl --user stop my-thing"
  severity = "danger"

# ── Settings (rendered into Settings → Plugins; substituted into commands)
[[setting]]
key         = "city"                       # `{{city}}` in commands
label       = "City"
type        = "string"                     # default
default     = "Istanbul"
description = "Used in the openweather URL."

[[setting]]
key     = "api_key"
label   = "API key"
type    = "secret"                         # stored in the system keyring
default = ""

[[setting]]
key     = "units"
label   = "Units"
type    = "choice"
choices = ["metric", "imperial"]
default = "metric"
```

### Field reference

| Field | Type | Required | Notes |
|---|---|---|---|
| `id` | string | ✓ | Kebab-case, no `/`, no `:` |
| `name` | string |  | Displayed in Settings + gallery |
| `version` | string | ✓ | Semver; the updater compares against the registry |
| `author` | string |  | Free-form |
| `min_mshell` | string |  | Empty = no floor |
| `description` | string |  | One paragraph; gallery + Settings |
| `icon` | string |  | Freedesktop icon name; gallery |
| `preview` | string |  | Screenshot URL (reserved for the gallery) |
| `entry` | string |  | Path to the WASM component, e.g. `"plugin.wasm"` |
| `entry_kind` | string |  | `"wasm"` |
| `[[widget]]` | array | 1+ | One bar pill per entry |
| `[[setting]]` | array | 0+ | User-facing controls; values substituted into commands |

### `[[widget]]` reference

| Field | Type | Notes |
|---|---|---|
| `key` | string | Bar widget name = `plugin:<plugin-key>:<widget-key>` |
| `icon` | string | Freedesktop icon name |
| `image` | string | Alternative: a path inside the plugin folder |
| `tooltip` | string | Substituted with settings |
| `label` | string | Static label; substituted |
| `exec` | string | Shell command; stdout → `{output}` |
| `template` | string | `"{output}"` or `"{icon} {output}"`, … |
| `interval` | int | Seconds; `0` = run once |
| `max_chars` | int | Truncate the rendered label |
| `opens_panel` | bool | Click opens the WASM panel (requires `entry`) |
| `on_click` | string | Left-click command (ignored if `opens_panel`) |
| `on_click_right` | string | Right-click command |
| `[[widget.menu]]` | array | Drop-down rows |

### `[[widget.menu]]` reference

| Field | Type | Notes |
|---|---|---|
| `label` | string | Row text |
| `icon` | string | Freedesktop symbolic |
| `exec` | string | Shell command run on click |
| `severity` | string | `"normal"` (default), `"warn"`, `"danger"` |

### `[[setting]]` reference

| Field | Type | Notes |
|---|---|---|
| `key` | string | `{{key}}` placeholder in commands |
| `label` | string | Form label |
| `type` | string | `"string"` (default), `"secret"`, `"number"`, `"bool"`, `"choice"` |
| `default` | string | Falls back if the user hasn't set one |
| `choices` | array | Required when `type = "choice"` |
| `description` | string | Help text below the field |

**Secrets**: when `type = "secret"`, the value is stored in the system
**keyring** (`org.freedesktop.secrets`, i.e. gnome-keyring or kde-wallet),
**never** in `plugins.toml`. Existing plaintext values from older installs
get auto-migrated into the keyring on first boot.

---

## 6. Declarative tier — pills, polls, menus

The whole tier is just the manifest. mshell handles everything else.

### A status pill

```toml
id = "uptime"
name = "Uptime"
version = "0.1.0"

[[widget]]
key      = "main"
icon     = "system-run-symbolic"
exec     = "uptime -p | sed 's/up //'"
template = "{output}"
interval = 60
```

### A pill with a drop-down menu

```toml
[[widget]]
key      = "main"
icon     = "network-vpn-symbolic"
exec     = "systemctl is-active wg-mullvad"
template = "{output}"
interval = 5

  [[widget.menu]]
  label = "Connect"
  icon  = "network-vpn-symbolic"
  exec  = "systemctl --user start wg-mullvad"

  [[widget.menu]]
  label    = "Disconnect"
  icon     = "network-offline-symbolic"
  exec     = "systemctl --user stop wg-mullvad"
  severity = "danger"
```

### Settings substituted into commands

```toml
[[widget]]
key      = "main"
exec     = "curl -s https://api.openweathermap.org/data/2.5/weather?q={{city}}&appid={{api_key}}"
interval = 900

[[setting]]
key     = "city"
label   = "City"
default = "Istanbul"

[[setting]]
key   = "api_key"
label = "API key"
type  = "secret"
```

When the user fills the form, mshell substitutes `{{city}}` and
`{{api_key}}` into the command. Secrets come straight from the keyring.

Built-in substitutions:

- `{{plugin_dir}}` — absolute path to your installed folder. Useful for
  shipping scripts: `exec = "sh {{plugin_dir}}/check.sh"`.
- `{output}` (single-brace, in `template`) — the `exec` command's stdout.
- `{icon}` (in `template`) — the widget's icon glyph.

### When to use this tier

- Anything a 1-line shell command can read.
- Toggles that map to `systemctl`, `ufw`, `swww`, … one-shots.
- Status indicators (battery, uptime, network, weather summary).

If you want **live interactive state** — sliders that immediately update a
gauge, a chat that streams tokens, lists you can filter and click into —
keep reading.

---

## 7. WASM tier — sandboxed panels

This is where you write **Rust** (or any wasm32 language) and get a full
in-shell GTK panel.

### The contract

The host (`mshell-plugin-host`) and your guest speak a generated component
interface defined in [`world.wit`](https://github.com/kenanpelit/margo/blob/main/mshell-crates/mshell-plugin-host/wit/world.wit).
The key idea:

- Your guest exports two functions: `view()` and `update(event)`.
- Both return a **flat list of nodes**. The renderer rebuilds the tree from
  the list, rooted at the node with id `"root"`.
- The host imports your capability calls: `log`, `notify`, `http`, … Each
  call is a single function in the WIT.

You don't write the protocol by hand. The `mplugin-sdk` gives you an
ergonomic builder (`El`) and a `Component` trait.

### Hello, panel

```rust
use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    static COUNT: RefCell<u32> = const { RefCell::new(0) };
}

struct Hello;

fn view_tree() -> El {
    let n = COUNT.with(|c| *c.borrow());
    El::vbox(vec![
        El::markdown(format!("**Hello** — clicked {n} times"))
            .class("plugin-hero"),
        El::button("bump", "Click me")
            .class("plugin-action plugin-action-primary"),
    ])
    .spacing(12)
    .padding(12)
    .class("plugin-panel-body")
}

impl Component for Hello {
    fn view() -> El {
        host::log(2, "hello: first render");
        view_tree()
    }
    fn update(ev: Event) -> El {
        if let EventKind::Click = ev.kind {
            if ev.id == "bump" {
                COUNT.with(|c| *c.borrow_mut() += 1);
            }
        }
        view_tree()
    }
}

export_component!(Hello);
```

Two pieces to internalise:

- **`view()` is pure-ish.** It reads `thread_local!` state and produces a
  tree. Don't put I/O here — the host calls it on the GTK main thread and
  any blocking host call (`http`, `run`, …) freezes the UI for that long.
- **`update(event)` is where mutation happens.** Any change to your
  `thread_local!` state goes here. After it returns, the renderer replays
  `view()`.

There is **no persistent state** across panel opens unless you `write_file`
it. The wasm instance is dropped when its cache slot is evicted.

---

## 8. The SDK in depth

### `El` builders

| Builder | Returns | Notes |
|---|---|---|
| `El::vbox(children)` | a vertical `gtk::Box` | |
| `El::hbox(children)` | a horizontal `gtk::Box` | |
| `El::scroll(children)` | a `gtk::ScrolledWindow` over a vbox | Floors `min-content-height` at 300 px |
| `El::grid(cols, children)` | a `gtk::Grid` flowing row-major | `cols` is the column count |
| `El::revealer(open, child)` | a `gtk::Revealer` | Pair with `.prop("transition", "slide-down")` |
| `El::stack(visible_id, children)` | a `gtk::Stack` | Children need `with_id`; `visible_id` switches |
| `El::label(text)` | a static `gtk::Label` | Selectable, wraps |
| `El::markdown(text)` | a bubble with `**bold** *italic* \`code\`` | Bubbles get a corner copy button automatically |
| `El::button(id, text)` | a `gtk::Button` echoing `Click` events tagged `id` | |
| `El::entry(id, text)` | a `gtk::Entry` echoing `Submit` on Enter | |
| `El::switch(id, on)` | a `gtk::Switch` echoing `Click` with `value="true"`/`"false"` | |
| `El::slider(id, min, max, value)` | a `gtk::Scale` echoing `Input` with the new number | Use `.prop("step", "0.5")` |
| `El::progress(fraction)` | a determinate `gtk::ProgressBar` | `fraction` is 0.0–1.0 |
| `El::image(src)` | a `gtk::Image` (icon name) or `gtk::Picture` (file path) | Symbolic icons get matugen-tinted |
| `El::separator()` | a horizontal `gtk::Separator` | |

### Layout properties

Every element gets the same chainable layout knobs:

| Method | Property key | Effect |
|---|---|---|
| `.padding(px)` | `padding` | Margin on all four sides |
| `.margin(px)` | `margin` | Margin on all four sides (alias) |
| `.spacing(px)` | `spacing` | Distance between children (vbox/hbox/scroll) |
| `.halign("start" / "center" / "end" / "fill")` | `halign` | GTK halign |
| `.valign(…)` | `valign` | GTK valign |
| `.hexpand(true)` | `hexpand` | Grab horizontal slack from siblings |
| `.vexpand(true)` | `vexpand` | Grab vertical slack from siblings |
| `.prop("width", "320")` | `width` | Minimum content width, px |
| `.prop("height", "240")` | `height` | Minimum content height, px |
| `.prop(k, v)` | (any) | Future-proof escape hatch |

Per-kind state goes through the same property bag — `properties["on"]`
for a switch, `properties["fraction"]` for a progress bar,
`properties["columns"]` for a grid. The builders set them for you.

### Ids

Interactive nodes (`button`, `entry`, `switch`, `slider`) **require** an
id; the renderer feeds it back to you on the matching `Event`. Layout/leaf
nodes (`vbox`, `label`, `markdown`, …) get a stable auto id (`n1`, `n2`,
…) unless you pin one with `.with_id("my-stable-id")`.

Pin an id when:

- You need the node to be the visible child of a `stack`
  (`El::stack("controls", vec![child.with_id("controls"), …])`).
- You want the renderer to preserve focus/selection across re-renders —
  matching ids let it diff stably.

### Design language classes

Every plugin gets the same matugen palette as the rest of the shell. Tag a
node with `.class("plugin-hero")` and it picks up the same accent surface
as the dns/vpn widgets, automatically themed.

| Class | When | Looks like |
|---|---|---|
| `plugin-panel-body` | Outermost vbox | Compact padding, base background |
| `plugin-hero` | Status header markdown | Surface-container-high card |
| `plugin-hero plugin-hero-on` | Active state | Primary-container accent |
| `plugin-action` | Big action button | 40 px, radius-sm |
| `plugin-action plugin-action-primary` | Filled accent | Primary background |
| `plugin-action plugin-action-danger` | Destructive | Error background |
| `plugin-toggle` | Calm toggle row | Surface-container-high |
| `plugin-toggle plugin-toggle-on` | On | Primary background |
| `plugin-row` | Selectable list row | Hover/press tints |
| `plugin-list` | Scroll of `plugin-row`s | Min-height floor |
| `plugin-search` | Search/filter entry | Tighter margins |
| `plugin-expand` | Anywhere | Sets GTK `hexpand` (CSS can't) |

`plugin-bubble-copy` is added automatically to the per-bubble copy button
the renderer overlays on `markdown` nodes — you don't apply it yourself.

### Events

`update(ev)` is called whenever:

| Source | `ev.kind` | `ev.id` | `ev.value` |
|---|---|---|---|
| Button click | `Click` | the button's id | `""` |
| Switch toggle | `Click` | the switch's id | `"true"` / `"false"` |
| Entry Enter | `Submit` | the entry's id | new text |
| Slider drag | `Input` | the slider's id | the new number formatted as `"%.4"` |
| `http-start` chunk | `StreamChunk` | the request id | a body slice (lossy utf-8) |
| `http-start` end | `StreamEnd` | the request id | `""` or `"error: …"` |
| `process-start` chunk | `StreamChunk` | the request id | stdout chunk |
| `process-start` end | `StreamEnd` | the request id | `""` or `"error: …"` |

Return whatever new `El` tree your state implies. Don't try to mutate in
`view()` — the host calls it on every event.

---

## 9. Host capabilities

Everything a plugin can reach outside its own memory is one of these
functions. The full WIT is at
[`mshell-plugin-host/wit/world.wit`](https://github.com/kenanpelit/margo/blob/main/mshell-crates/mshell-plugin-host/wit/world.wit).

### Logging

```rust
host::log(2, "anything: a message");
```

Levels: `0` trace, `1` debug, `2` info, `3` warn, `4` error. Visible in the
mshell journal when `RUST_LOG` includes your plugin's target.

### Settings

```rust
let api_key = host::get_setting("api_key");   // empty string if unset
```

Reads the user's value for a declared `[[setting]]`. Secret settings come
straight from the keyring.

### Notifications

```rust
host::notify("My plugin", "Done.");
```

Posts to `notify-send`. Best-effort — fails silently if there's no
notification daemon.

### Clipboard

```rust
host::copy("text to copy");
let pasted = host::clipboard_read();   // "" on any failure
```

Backed by `wl-copy` / `wl-paste`. Wayland only.

### Scoped filesystem

```rust
host::write_file("counter.txt", b"42")?;       // returns Result<(), String>
let bytes = host::read_file("counter.txt")?;   // returns Result<Vec<u8>, String>
```

Both are scoped to `~/.local/share/mshell/plugins/<plugin-id>/`. Absolute
paths and `..`/path-traversal are rejected. Use this for a todo file, a
cache, a layout snapshot — anything that should survive across panel
opens.

Writes are atomic-ish: the host writes a tmp file and renames into place,
so a crash mid-write doesn't corrupt the existing file.

### Subprocess (blocking)

```rust
let out = host::run("mullvad", &["status".to_string()]);
println!("{}", out.stdout);
println!("exit: {}", out.code);   // -1 if spawn failed
```

Blocks the UI until the process exits. Use only for fast commands
(`status`, `toggle`, …). For long-running output, use `process-start`
instead.

The host passes `program` straight to `std::process::Command::new` — **no
shell**, so glob/redirect/etc. don't apply.

### Subprocess (streaming)

```rust
let req_id = host::process_start("journalctl", &["-fu".into(), "mshell".into()]);
// stdout chunks arrive as StreamChunk events with id == req_id.
```

Mirror of `http-start` for processes. The child's stdout is read on a
worker thread; each chunk arrives in your `update(ev)` as
`EventKind::StreamChunk` with `ev.id == req_id`. One terminal
`StreamEnd` event marks the process exit (or `value` carries an
`error: …` message).

When the cache slot for your plugin is evicted, the child gets killed.

### HTTP (blocking)

```rust
use mplugin_sdk::host::HttpRequest;

let resp = host::http(&HttpRequest {
    method: "GET".into(),
    url: "https://example/json".into(),
    headers: vec![],
    body: String::new(),
})?;
println!("{}: {}", resp.status, resp.body);
```

Returns `Result<HttpResponse, String>`. Blocks the UI. Use for short
requests.

### HTTP (streaming)

```rust
let req_id = host::http_start(&HttpRequest { …, url: sse_url, … });
// StreamChunk events deliver body slices keyed by req_id until StreamEnd.
```

For server-sent events, chunked responses, or any body big enough to
matter. The host worker thread reads the response off the UI thread.

---

## 10. Settings & secrets

Declare settings in the manifest, read them in the plugin:

```toml
[[setting]]
key     = "model"
label   = "Model"
type    = "choice"
choices = ["gemini-2.5-flash", "gemini-2.5-pro"]
default = "gemini-2.5-flash"

[[setting]]
key   = "api_key"
label = "API key"
type  = "secret"
```

```rust
let model = host::get_setting("model");
let api_key = host::get_setting("api_key");   // from the keyring
```

The shell renders the form in **Settings → Plugins → Installed →** your
plugin's gear. Form changes are saved as the user types (debounced); on
the next `view()` the new value is in.

### Secret handling

Set `type = "secret"`. The Settings form shows the value as `••••`,
stores it in the system keyring (`org.freedesktop.secrets`), and
`plugins.toml` never sees it. You read it through `host::get_setting`
the same way you read any other setting — the mapping is transparent.

If your plugin had plaintext secrets in `plugins.toml` from before this
feature shipped, mshell migrates them into the keyring on the next boot
and removes them from the TOML.

To audit the keyring, run `secret-tool search service mshell-plugin`.

---

## 11. State & persistence

### In-memory state

Each panel instance has one wasm linear memory. Use `thread_local!` —
wasm components are single-threaded:

```rust
thread_local! {
    static FILTER: RefCell<String> = const { RefCell::new(String::new()) };
    static ITEMS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
```

The host calls `view()` and `update()` on the same logical thread, so no
locking. State survives across renders within a single panel-open
lifetime.

### Persistence across opens

State is **lost** when the host evicts your panel from its cache (on
restart, on `mshellctl plugin reload`, on the file watcher firing).
Anything that should survive needs `host::write_file`:

```rust
fn save(items: &[String]) {
    let _ = host::write_file("items.json", serde_json::to_vec(items).unwrap().as_slice());
}

fn load() -> Vec<String> {
    host::read_file("items.json")
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
```

The data dir is per-plugin — your todo list can't read another plugin's
notes file.

---

## 12. The development loop

### File-watcher hot reload

mshell watches every installed plugin's directory. The moment a
`plugin.wasm` mtime changes, the cached panel is evicted (debounced
300 ms) and re-instantiates on the next open. **No mshell restart, no
explicit reload command.**

The recommended dev loop:

```sh
cargo watch -i "*.wasm" -i "target/**" -s '
  cargo build --target wasm32-wasip2 --release \
    && cp target/wasm32-wasip2/release/my_plugin.wasm \
          ~/.config/margo/mshell/plugins/my-plugin/plugin.wasm
'
```

Or, if you don't want `cargo-watch`, do the build + copy by hand. Either
way: rebuild → reopen panel → the change is on screen.

### Manual reload

If something gets stuck (or you're debugging the watcher itself):

```sh
mshellctl plugin reload my-plugin
```

This bypasses the watcher and forces an eviction.

### Logging

mshell uses `tracing` with an `EnvFilter`. Plugin host messages live
under `mshell_plugin_host`. To see your plugin's `host::log(2, …)`
output:

```sh
systemctl --user edit mshell
```

Add:

```
[Service]
Environment=RUST_LOG=mshell_plugin_host=info,mshell=info
```

`systemctl --user restart mshell`, then tail with `journalctl --user -fu
mshell`.

### Debugging the rendered tree

Add a temporary `host::log(2, &format!("nodes: {nodes:?}"))` line in
your guest, or set `GTK_DEBUG=interactive` in the systemd env to open
the GTK inspector — your panel's widgets are all there.

---

## 13. The design language

A plugin that wants to look like the rest of the shell doesn't have to
think about colours. Apply the class taxonomy in §8 and matugen does
the rest. The full design spec is in
[`mshell-crates/mshell-frame/DESIGN.md`](https://github.com/kenanpelit/margo/blob/main/mshell-crates/mshell-frame/DESIGN.md);
read it before you start naming custom CSS classes.

The big rules:

- **Never hardcode a colour.** Use the matugen tokens (`var(--primary)`,
  `var(--surface-container-high)`, …) through the class taxonomy.
- **Mind the calm/warn/danger ladder.** The `severity = "danger"` row
  on a declarative menu, the `plugin-action-danger` button, the
  `plugin-toggle-on` accent — pick the right one for the meaning, not
  the look.
- **Respect the radius + spacing scales.** Don't invent a new `padding:
  9px`; `.spacing(px)`/`.padding(px)` must land on the **4 / 8 / 12 / 16
  / 24 / 32** scale (1–2px hairline is the only sub-4 exception). The
  renderer accepts any value, but off-scale gaps make a plugin read as
  "not part of the shell".
- **Per-node inline styles are explicitly out of scope.** If you want
  something the design language doesn't have, that's a renderer / SDK
  feature request — file an issue on the [roadmap](road_map.md) board.

### Self-lint before you publish (the plugin half of DESIGN.md §15)

Plugins ship no SCSS, so the shell's grep gate doesn't cover them —
**run these over your `src/` yourself.** A clean plugin returns nothing:

```sh
# No hardcoded colour anywhere in the node tree (use the class taxonomy).
grep -rnE '#[0-9a-fA-F]{3,8}\b' src/
# Spacing/padding must be on-scale; this flags off-scale gaps to review.
grep -roE '\.(padding|spacing)\([0-9]+\)' src/ | sort -u
#   → every value should be one of 0/2/4/8/12/16/24/32 (2 = hairline).
```

A panel reading async data (`host::run`, `http_*`, `system-state`) must
also define its **non-happy states** (DESIGN.md §17) — never a blank or
reflowing panel:

| State | Render |
|---|---|
| **Loading** | a calm placeholder line / spinner, not an empty box |
| **Empty** | a dim centred line ("Nothing yet"), no error styling |
| **Error** | an inline `plugin-action-danger`-tinted message + a Retry |
| **No service** | a dim "X not installed / not running" line (informational, not red) |

And the accessibility floor (DESIGN.md §13.8): an **icon-only**
button/row sets a label or tooltip describing the *action* ("Reconnect",
"Delete") — the glyph is not a name. Reordering uses up/down buttons
(the WASM renderer has no drag gestures); keep them keyboard-reachable.

> Same spirit as the shell's CI lint, run by hand: a plugin that passes
> these greps + states + labels looks and behaves like it always
> belonged. The current first-party set passes (zero hardcoded hex,
> spacing on-scale).

---

## 14. Publishing

### Your own source

A *source* is any git repo with a `registry.toml` at its root:

```
my-margo-plugins/
├── registry.toml
└── hello/
    ├── manifest.toml
    └── plugin.wasm
```

`registry.toml` lists every plugin in the repo:

```toml
[[plugins]]
id          = "hello"
dir         = "hello"            # folder name
version     = "0.1.0"            # must match manifest
name        = "Hello"
min_mshell  = "0.9.4"
description = "One sentence."
icon        = "applications-engineering-symbolic"
```

Push to GitHub (or any git host the user can clone). Users add your
source in Settings → Plugins → Sources by name + URL, hit Refresh, and
install with one click.

### Auto-update

If the user picks **Settings → Plugins → Updates → On login**, mshell
fetches every source's `registry.toml` about a minute after login and
re-installs any plugin whose registry version is newer than the
installed one (uses standard semver via `is_newer`). The widget's bar
pill keeps working while it updates; a toast announces what changed.

Bump the registry's `version` field (and the manifest's, so they match)
when you ship a new build.

### Contributing here

To add a plugin to the **official** source (this repo):

1. Land it in a feature branch under its own folder.
2. Add a `[[plugins]]` row to `registry.toml`.
3. Open a PR. The maintainers review the manifest + source.

The official source's URL is hard-coded into mshell, so plugins here
benefit from a stripped, key-collision-free name (just `mullvad`, not
`a1b2c3:mullvad`).

---

## 15. Worked examples

The three plugins shipping in this repo each demonstrate a different
shape; reading their source is the fastest way to absorb the SDK.

### `demo-kit` — the reference

Exercises **every** node kind and **every** host capability. Read its
`src/lib.rs` first if you want a tour:

- 2×2 image grid via `El::grid`
- Stack with Controls / About tabs
- Revealer wrapping the filesystem + clipboard sections
- Switch, slider, progress driven from one shared `thread_local!`
- `read_file` / `write_file` round-trip via "Save" / "Re-load"
- `clipboard_read` round-trip via "Paste"

### `mullvad` — rich VPN control via `host::run`

A real-world plugin that wraps the `mullvad` CLI. Demonstrates:

- A hero card showing live state (`Connected — de-ber-wg-006`)
- Filled primary / error-tinted secondary action buttons via the
  design-language classes
- Searchable scrollable country list with relay counts
- Synchronous shell-outs to the CLI through `host::run`
- All state in `thread_local!`, all rendering through `.class("plugin-…")`

### `assistant-panel` — streaming chat via `http_start`

The Gemini chat plugin. Demonstrates:

- A streaming HTTP request (`http_start`) producing `StreamChunk`
  events that accumulate into a markdown bubble
- Secret settings (`api_key`) read from the keyring
- A chat log of `El::markdown` bubbles in an `El::scroll`
- The corner copy button is automatic; right-click also copies; Ctrl+C
  copies the active selection or the whole conversation

---

## 16. Troubleshooting

### My plugin doesn't show up under Installed

Check `~/.config/margo/mshell/plugins/<key>/manifest.toml` is present
and parses (`toml --check <path>` if you have one). Check the journal:
`journalctl --user -u mshell | grep plugin`.

### `mshellctl menu plugin <key>` opens nothing / opens the wrong plugin

The key is matched against the plugin's composite key, the widget key,
and the full `plugin:<comp>:<widget>` name. For the official source's
`mullvad` plugin with `[[widget]] key = "vpn"`, all of these work:

- `mshellctl menu plugin mullvad`
- `mshellctl menu plugin vpn`
- `mshellctl menu plugin mullvad:vpn`

If two enabled plugins share the same widget key, the first one wins.
Rename one to disambiguate.

### Panel opens but renders empty

The most common cause: your `plugin.wasm` is from before a protocol
break. Rebuild against the current `mplugin-sdk`. The host logs `load
failed: …` in the journal when it can't instantiate.

### Build error: `cannot find function process_start in mod host`

You're building against an older SDK. Either:

- Bump the `mplugin-sdk` git ref in your `Cargo.toml`.
- Or vendor the SDK if you need a specific commit.

### The wasm is huge (200 KB+)

`opt-level = "s"` is in the template by default. Also try:

- `lto = "fat"` in `[profile.release]`.
- `codegen-units = 1`.
- `panic = "abort"`.
- `wee_alloc` as the global allocator.

A reasonable target is 30–80 KB for a small plugin.

### Hot reload doesn't fire

The watcher only sees plugins that were installed when mshell started.
Newly installed plugins aren't watched until the next restart. Run
`mshellctl plugin reload <key>` once after the first install — it's the
manual fallback.

### `host::read_file` returns "disallowed path component"

You passed an absolute path or one containing `..`. Use relative paths
only; the scope is `~/.local/share/mshell/plugins/<plugin-id>/`.

### Secrets show as empty after upgrade

The migration only moves values that were in `plugins.toml`. If your
keyring was wiped (a fresh install on the machine) you need to re-enter
the value through Settings → Plugins → your plugin's gear.

---

## Appendix A — full host WIT

The single source of truth for the protocol is
[`mshell-crates/mshell-plugin-host/wit/world.wit`](https://github.com/kenanpelit/margo/blob/main/mshell-crates/mshell-plugin-host/wit/world.wit).
A compact summary of the node + event types:

```wit
enum node-kind {
    vbox, hbox, label, button, entry, scroll, markdown,
    image, switch, slider, progress, separator,
    grid, revealer, stack,
}

record node {
    id: string,
    kind: node-kind,
    text: string,
    children: list<string>,
    class: string,
    properties: list<tuple<string, string>>,
}

enum event-kind {
    click, input, submit, stream-chunk, stream-end,
}

record event {
    id: string,
    kind: event-kind,
    value: string,
}
```

Host capability surface:

```wit
log:               func(level: u32, message: string);
get-setting:       func(key: string) -> string;
notify:            func(summary: string, body: string);
copy:              func(text: string);
clipboard-read:    func() -> string;
read-file:         func(rel-path: string) -> result<list<u8>, string>;
write-file:        func(rel-path: string, bytes: list<u8>) -> result<_, string>;
run:               func(program: string, args: list<string>) -> process-output;
http:              func(req: http-request) -> result<http-response, string>;
http-start:        func(req: http-request) -> string;
process-start:     func(program: string, args: list<string>) -> string;
```

---

## Appendix B — SDK API

`mplugin-sdk` is a thin wrapper over the generated WIT bindings. The
public surface:

```rust
// Builder for the node tree — see §8 for the full table of methods.
pub struct El { /* … */ }
impl El {
    pub fn vbox(children: Vec<El>) -> El;
    pub fn hbox(children: Vec<El>) -> El;
    pub fn scroll(children: Vec<El>) -> El;
    pub fn grid(columns: u32, children: Vec<El>) -> El;
    pub fn revealer(revealed: bool, child: El) -> El;
    pub fn stack(visible_id: impl Into<String>, children: Vec<El>) -> El;
    pub fn label(text: impl Into<String>) -> El;
    pub fn markdown(text: impl Into<String>) -> El;
    pub fn button(id: impl Into<String>, text: impl Into<String>) -> El;
    pub fn entry(id: impl Into<String>, text: impl Into<String>) -> El;
    pub fn switch(id: impl Into<String>, on: bool) -> El;
    pub fn slider(id: impl Into<String>, min: f64, max: f64, value: f64) -> El;
    pub fn progress(fraction: f64) -> El;
    pub fn separator() -> El;
    pub fn image(src: impl Into<String>) -> El;

    pub fn with_id(self, id: impl Into<String>) -> El;
    pub fn class(self, class: impl Into<String>) -> El;
    pub fn prop(self, key: impl Into<String>, value: impl Into<String>) -> El;
    pub fn padding(self, px: i32) -> El;
    pub fn margin(self, px: i32) -> El;
    pub fn spacing(self, px: i32) -> El;
    pub fn halign(self, align: impl Into<String>) -> El;
    pub fn valign(self, align: impl Into<String>) -> El;
    pub fn hexpand(self, expand: bool) -> El;
    pub fn vexpand(self, expand: bool) -> El;
}

// Implement this on your plugin's marker type, then `export_component!`.
pub trait Component {
    fn view() -> El;
    fn update(ev: Event) -> El;
}

// Re-exports for the wire types and host capability functions.
pub use Event;
pub use EventKind;     // Click / Input / Submit / StreamChunk / StreamEnd
pub mod host {
    pub fn log(level: u32, message: &str);
    pub fn get_setting(key: &str) -> String;
    pub fn notify(summary: &str, body: &str);
    pub fn copy(text: &str);
    pub fn clipboard_read() -> String;
    pub fn read_file(rel_path: &str) -> Result<Vec<u8>, String>;
    pub fn write_file(rel_path: &str, bytes: &[u8]) -> Result<(), String>;
    pub fn run(program: &str, args: &[String]) -> ProcessOutput;
    pub fn http(req: &HttpRequest) -> Result<HttpResponse, String>;
    pub fn http_start(req: &HttpRequest) -> String;
    pub fn process_start(program: &str, args: &[String]) -> String;
    pub struct HttpRequest { /* method, url, headers, body */ }
    pub struct HttpResponse { /* status, body */ }
    pub struct ProcessOutput { /* stdout, stderr, code */ }
}

// The macro that wires everything to the WIT-generated `export!`.
#[macro_export]
macro_rules! export_component { ($component:ty) => { /* … */ }; }
```

Read the source at
[`mplugin-sdk/src/lib.rs`](https://github.com/kenanpelit/margo/blob/main/mplugin-sdk/src/lib.rs)
for the canonical, doc-commented version.

---

Happy hacking. Bugs, missing capabilities, surprising behaviour →
[file an issue on the margo repo](https://github.com/kenanpelit/margo/issues),
or write your fix and open a PR.
