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

### M1 — Manager core (`mshell-plugins` crate)
- [ ] `manifest.toml` + `registry.toml` serde types
- [ ] Fetch a source's registry via shallow sparse git clone
- [ ] Install / uninstall / update a plugin (sparse-clone its folder into
      `~/.config/margo/mshell/plugins/<key>/`)
- [ ] Local state in `plugins.toml` (sources list + enabled plugins)
- [ ] Manifest validation + `min_mshell` version gate
- [ ] Composite keys (`hash:id`) so custom sources don't collide with official
- [ ] Unit tests

### M2 — Widget bridge
- [ ] Map each enabled plugin's `[[widget]]` to a `CustomWidgetConfig`
- [ ] Namespace placed widgets as `plugin:<id>:<key>`
- [ ] Make plugin widgets selectable in bar slots like any built-in widget
- [ ] Resolve manifest asset paths (icons/images) relative to the plugin dir

### M3 — Settings UI
- [ ] **Plugins** page with **Sources / Available / Installed** sub-tabs
- [ ] Add / remove source URLs
- [ ] Browse available, install, enable/disable, update, remove
- [ ] Install/enable confirmation that lists the shell commands a plugin runs

### M4 — CLI + official content
- [ ] `mplugins` CLI: `install <url|id>`, `list`, `update`, `remove`, `sources`
- [ ] Publish the first real plugins in this repo + flesh out `registry.toml`
- [ ] End-to-end docs (authoring, publishing, installing)

### M5 — Beyond declarative (future, optional)
- [ ] Declarative **menu** spec (a popup the pill opens: rows, toggles, sliders)
- [ ] Richer widget vocabulary (progress, dropdown, multi-line)
- [ ] Per-plugin settings surfaced in the Settings page
- [ ] Optional embedded **Lua** API for logic-rich widgets
- [ ] (Stretch) sandboxed **WASM** plugins for arbitrary UI

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
