# mplugins — Roadmap

> **mplugins** is the plugin system for the [margo](https://github.com/kenanpelit/margo)
> desktop shell (mshell): install widgets from external git repositories
> straight from the shell, without recompiling. This roadmap tracks the
> initiative across `margo` and `margo-plugins`.

## Vision

A plugin system that is **safer by default** than QML/JavaScript shells
(Quickshell, Noctalia) — true capability-based sandboxing, language-agnostic
guests, versioned protocol — **while matching their authoring ergonomics and
UI expressiveness**. Today we hold the safety lead. The work below is what
closes the rest.

## Where we are (2026-05-29)

- Two tiers: **declarative** (`manifest.toml` + shell exec) and **WASM**
  (wasmtime 27 component-model, wasm32-wasip2 guests).
- Host capability set: `log`, `get-setting`, `notify`, `copy`, `http`,
  `http-start` (streaming), `run` (subprocess).
- Node protocol: `id`, `kind` (vbox / hbox / label / button / entry / scroll
  / markdown), `text`, `children`, `class` (design-language hook).
- Generic IPC: `mshellctl menu plugin <key>` resolves any installed plugin
  with no per-plugin code.
- Settings → Plugins ships sources / registry / install / per-plugin gear
  (panel size + position) and an Updates card (Off / On login + Update-all).
- Three plugins live: `assistant-panel`, `mullvad`, `network-speed`.

## Where Noctalia is still ahead

| Gap | Why it matters | Closable? |
|---|---|---|
| **Iteration speed** — QML hot reload vs. our cargo-build-then-copy loop | Authoring is the slowest path to ecosystem growth | Yes (hot reload + reload IPC) |
| **UI expressivity** — only 7 node kinds; no images, switches, sliders, animations | Plugins look "stock"; can't deliver custom polish | Yes (more node kinds + property bag) |
| **Author ergonomics** — minimal SDK, no template, scarce examples | Lifts the floor for new authors | Yes (SDK 1.0 + cookiecutter) |
| **Ecosystem size** — 3 plugins vs. a long catalogue | Perception of maturity | Yes, but bounded by content work |
| **Fully custom rendering** — QML shaders / particles / canvas | A long tail few plugins actually use | **No, and we won't try** — see "Won't do" |

---

## Now — the milestone we're shipping next

### N1. Future-proof the protocol with a property bag

Add `properties: list<tuple<string, string>>` to the node record. Every new
extension (layout, image sources, switch state, slider range, animation
hints, …) goes through this map. **No more node-record breaks after this
one.** This is the foundation; all other "Now" items lean on it.

- Update `host/wit/world.wit` + `mplugin-sdk/wit/world.wit`.
- SDK: `El::prop(key, value)` plus typed shortcuts (`.padding(8)`, etc.).
- Renderer: read property bag, apply via GTK setters / CSS.
- One protocol bump → rebuild every fixture and every plugin once.

### N2. New node kinds for real UIs

Additive enum extensions — old guests keep working, new hosts gain new
arms:

- `image` — file path or URL → `gtk::Picture` (matugen-tinted symbolic for
  `*-symbolic` icons).
- `switch` — bool state via `properties["on"]`, click toggles + echoes an
  event.
- `slider` — `properties["min"|"max"|"value"|"step"]`, drag echoes.
- `progress` — `properties["fraction"]` 0.0–1.0.
- `separator` — visual divider that respects design spacing.

Acceptance: a demo guest renders all five and the standard chat panel still
works.

### N3. Node-level layout via property bag

No more "tag with a CSS class to set hexpand" workarounds. As first-class
properties on the bag:

- `padding`, `margin`, `spacing` (px integers).
- `halign`, `valign` (`start | center | end | fill`).
- `hexpand`, `vexpand` (`true | false`).
- `width`, `height` (min content).

Acceptance: mullvad's panel rebuilt against properties (not the
`plugin-expand` class) and looks identical or better.

### N4. New host capabilities (the high-impact subset)

- `timer-schedule(id: string, delay-ms: u32)` → fires a `TimerEvent` after
  the delay. Lets a plugin re-render without busy-waiting.
- `clipboard-read() -> string` — we already have `copy`; round-trip the
  clipboard.
- `read-file(rel-path: string) -> result<list<u8>, string>` — scoped to the
  plugin's install dir + a `plugin-data-dir` (`~/.local/share/mshell/plugins/<key>/`).
- `write-file(rel-path: string, bytes: list<u8>) -> result<_, string>` —
  same scope. Enables todo, notes, kanban, cache.

Out of scope for this milestone (queued for Next): keybind register,
process-stream, media-now-playing, system-state.

### N5. `mshellctl plugin reload <key>` for fast iteration

Evicts the cached `PluginPanel`, re-reads the manifest, re-instantiates from
disk on next open. Wire your `cargo watch` to call it and you have a
~2-second edit-to-pixels loop **without restarting mshell**.

Acceptance: editing mullvad's `view`, rebuilding the wasm, calling
`mshellctl plugin reload mullvad`, and re-opening the panel shows the
change.

### Now — exit criteria

- All five Now items merged into `main` and built into a `margo-git`
  package.
- WIT bump is the *last* one that breaks pre-compiled guests (the property
  bag absorbs everything thereafter).
- Every shipped plugin (`assistant-panel`, `mullvad`, `network-speed`)
  rebuilt against the new SDK and re-published.
- A new `demo-kit` plugin in `margo-plugins` exercises every new node kind.

---

## Next — after Now ships

- **File-watcher hot reload** layered on top of N5. `inotify` watches each
  enabled plugin's `plugin.wasm`; mtime change → automatic reload.
  No more `mshellctl plugin reload` step.
- **More node kinds**: `grid`, `revealer` (with `motion-*` transition
  classes), `stack`, `picture` (aspect-fit).
- **More host capabilities**:
  - `keybind-register` — plugin asks the compositor for a global hotkey,
    margo dispatches it back as an event.
  - `process-stream` — like `http-start` but for subprocess stdout (live
    output from `journalctl -f`, `tail -f`, etc.).
  - `media-now-playing` — typed read of the active player (track, artist,
    art URL, position) via the wayle media service.
  - `system-state` — battery, network, idle (also via wayle).
- **SDK 1.0**: semver discipline, `wee_alloc` + LTO so a typical plugin is
  ~20–30 KB instead of 120–190 KB, a `protocol-version` host call for
  defensive guests.
- **`cargo generate margo-plugin`** template — `manifest.toml` + `Cargo.toml`
  + a working skeleton in one command.
- **Plugins gallery** in Settings → Plugins: card layout for the registry
  view with the plugin's icon, name, short description, screenshot URL (new
  `preview` field in `registry.toml`), and an Install button. The current
  list view stays as a fallback.

---

## Later — the long tail that turns the perception around

- **First-party plugin set** (one per ~day): `calendar` (events + month
  view), `todo` (file-backed via `write-file`), `weather-detailed`, `github-prs`,
  `rss`, `kanban`, `pomodoro`.
- **Community marketplace UI** (ratings + screenshots in the gallery).
- **Plugin author guide** under `margo-plugins/AUTHORING.md` plus a tutorial
  walking through a "hello → http → settings → panel" arc.
- **Optional scripting tier** — only if N5 + file watcher still aren't
  enough. A QuickJS-embedded sub-tier (`entry_kind = "js"`) lets you skip
  the wasm build for prototypes. Costs us a second runtime and a partial
  sandbox story, so we only do it if the data says we have to.

---

## Won't do

- **Per-node inline CSS / styles.** Bypassing the design language tier was
  the explicit reason matugen tokens exist. The richness we want comes from
  more node kinds + properties, not from letting plugins paint over the
  shell.
- **Custom shaders, raw canvas drawing, custom render layers.** The long
  tail of effects that QML shells can do but most don't actually use. The
  runtime cost of catching up here would dwarf the benefit.
- **Loading arbitrary native libraries.** Plugins are WASM components or
  shell commands — never `dlopen`. The safety story depends on it.

---

## Measuring progress

The "Now" milestone is done when **all** are true:

1. A new author can run `cargo generate margo-plugin`, edit a `view`, run
   `cargo build --target wasm32-wasip2 --release && mshellctl plugin reload <key>`,
   and see the change in **under 5 seconds**.
2. `demo-kit` renders every node kind from N2 + every layout property from
   N3 + uses every new host capability from N4.
3. The Plugins gallery shows preview cards (Next milestone bleed-over, but
   the registry schema gains `preview` here).
4. `mullvad` and `assistant-panel` are rebuilt using `properties` for
   layout — no more `plugin-expand`-as-class workaround.
