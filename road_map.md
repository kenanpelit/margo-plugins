# mplugins — Roadmap

**mplugins** is the plugin system for the [margo](https://github.com/kenanpelit/margo)
desktop shell (mshell): install widgets from external git repositories,
straight from the shell, without recompiling.

This document tracks the initiative across its repos and crates. It will
evolve as milestones land.

## Naming

| Layer | Name | Where it lives |
|---|---|---|
| Registry / content repo | `margo-plugins` | this repo |
| Manager crate | `mshell-plugins` | margo repo (`mshell-crates/`) |
| CLI | `mplugins` | margo repo (top-level binary) |
| Settings surface | **Settings → Widgets → Plugins** | margo repo (`mshell-settings`) |

## Design in one paragraph

mshell is compiled Rust + GTK4, so — unlike QML-based shells — it cannot load
arbitrary downloaded UI code live. mplugins therefore ships **declarative**
plugins: a plugin is a `manifest.toml` (plus optional assets) describing one
or more bar widgets (icon, label, a command to poll for the label, click
actions). The manager git-clones plugins from *source* repos and feeds their
widget definitions into mshell's existing custom-widget engine. No recompile,
no native ABI, language-agnostic, and no less safe than any status-command
widget.

## Milestones

### M0 — Registry scaffold ✅
- [x] `margo-plugins` repo: README, `registry.toml` skeleton, GPL-3.0 license
- [x] Documented source / registry / manifest layout

### M1 — Manager core (`mshell-plugins` crate) ✅
- [x] `manifest.toml` + `registry.toml` serde types
- [x] Fetch a source's registry via shallow sparse git clone
- [x] Install / uninstall a plugin (sparse-clone its folder into
      `~/.config/margo/mshell/plugins/<key>/`)
- [x] Local state in `plugins.toml` (sources list + enabled plugins)
- [x] Manifest validation + `min_mshell` version gate
- [x] Composite keys (`hash:id`) so custom sources don't collide with official
- [x] Unit tests (18, green)
- [ ] Update (re-install on newer registry version) — deferred to M4 alongside the CLI

### M2 — Widget bridge ✅
- [x] Map each enabled plugin's `[[widget]]` to a `CustomWidgetConfig`
- [x] Namespace placed widgets as `plugin:<key>:<widget>` (derived layer:
      added on config load, stripped before persist — profile stays clean)
- [x] Plugin widgets render through the existing `Custom(name)` bar path
- [x] Resolve manifest image paths relative to the plugin dir

### M3 — Settings UI ✅ (core)
- [x] **Plugins** page under Settings → Widgets (Sources / Available /
      Installed sections)
- [x] Add / remove source URLs
- [x] Refresh registries, install, enable/disable, uninstall (async git off
      the GTK loop)
- [x] Trust gate: installed rows list the shell commands a plugin declares;
      code only runs once you enable it
- [ ] `update` (re-install on newer version) — with the CLI in M4
- [ ] Polish pass after live testing (empty/error states, busy spinner)

### M4 — CLI + official content
- [ ] `mplugins` CLI: `install <url|id>`, `list`, `update`, `remove`, `sources`
- [ ] Publish the first real plugins in this repo + flesh out `registry.toml`
- [ ] End-to-end docs (authoring, publishing, installing)

### M5a — Declarative menus + settings ✅
- [x] `[[widget.menu]]` spec: a click-popover of command rows (icon + label →
      `sh -c`), rendered from manifest data. Unblocks "pill + actions menu"
      widgets (firewall, containers, …) with no compiled code.
- [x] `[[setting]]` spec: per-plugin user settings (string / **secret** /
      number / bool / choice) with a gear → inline form in Settings; values
      substitute into commands via `{{key}}` (secrets stored `0600`).
- [ ] Richer rows — toggles/sliders bound to live state (needs the WASM tier)

> The declarative tier is now feature-complete: pill + label/exec + menus +
> settings. Anything beyond this (arbitrary/interactive UI, the assistant
> *panel*) is the WASM tier below.

### M5b — Sandboxed Rust/WASM plugins (chosen scripting tier)
- [ ] Embed a wasm runtime (wasmtime) + a host API for building widgets/menus
- [ ] Plugin authored in Rust (or any lang) → compiled to wasm → run sandboxed
- [ ] Live/interactive state, per-plugin settings, richer UI
- This is the big one (host UI/component model from scratch); tackled after
  the declarative tier + M4 settle.

> Decision (2026-05-29): the scripting tier is **WASM (Rust plugins)**, not
> Lua — sandboxed, language-agnostic, no native-ABI fragility.

## Open decisions

- **Format:** TOML for `manifest.toml` and `registry.toml` (Rust-idiomatic,
  comments) — open to JSON for noctalia-ecosystem familiarity.
- **CLI scope:** ship `mplugins` in M1, or start UI-only and add the CLI in M4.
- **Trust model:** install-time confirmation showing declared commands; plugins
  run with the user's privileges (same as any custom-widget `exec`).

## Explicitly out of scope (for now)

Arbitrary downloaded GTK UI, novel widget types, or custom settings panels —
those require the Lua/WASM tiers in M5. Declarative plugins cover the common
status / info / action widgets.

## Security

Plugins run shell commands (`exec` / `on_click`) as your user. Install only
plugins you trust and review the commands a plugin declares before enabling.
