//! Color Scheme Creator — preset manager + hex editor for margo's
//! `~/.config/margo/colors.conf`.
//!
//! Port of the noctalia plugin scoped to margo's 7-token compositor palette
//! (focuscolor / bordercolor / urgentcolor / scratchpadcolor / globalcolor /
//! overlaycolor / shadowscolor). Saves named snapshots under
//! `~/.config/margo/colorschemes/<name>/colors.conf`, applies one with a
//! `cp + mctl reload` shell-out via `host::run`.
//!
//! Caveat: margo regenerates `colors.conf` from the matugen wallpaper
//! pipeline every time the wallpaper changes — applying a hand-curated
//! preset persists only until the next regeneration. The panel surfaces
//! this caveat as dim help text.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;
use std::collections::BTreeMap;

const TOKENS: &[(&str, &str)] = &[
    ("focuscolor", "Focused window border"),
    ("bordercolor", "Unfocused window border"),
    ("urgentcolor", "Urgent toplevel"),
    ("scratchpadcolor", "Scratchpad border"),
    ("globalcolor", "Tag-global border"),
    ("overlaycolor", "Overlay surface"),
    ("shadowscolor", "Window drop shadow"),
];

thread_local! {
    /// In-progress edits — hex string per token.
    static EDITS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static PRESETS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NEW_NAME: RefCell<String> = const { RefCell::new(String::new()) };
    static STATUS: RefCell<String> = const { RefCell::new(String::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

fn ensure_loaded() {
    LOADED.with(|l| {
        if *l.borrow() {
            return;
        }
        *l.borrow_mut() = true;
        load_current();
        load_presets();
    });
}

/// Read `~/.config/margo/colors.conf` into the EDITS map.
fn load_current() {
    let out = host::run(
        "sh",
        &["-c".into(), "cat ~/.config/margo/colors.conf".into()],
    );
    let mut map = BTreeMap::new();
    for line in out.stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim().to_string();
        let val = line[eq + 1..].trim().to_string();
        if TOKENS.iter().any(|(t, _)| *t == key) {
            map.insert(key, val);
        }
    }
    // Fill in anything still missing.
    for (k, _) in TOKENS {
        map.entry((*k).into()).or_insert_with(|| "0x000000ff".into());
    }
    EDITS.with(|e| *e.borrow_mut() = map);
}

/// List subdirectories under `~/.config/margo/colorschemes/` — each one is a
/// preset.
fn load_presets() {
    let out = host::run(
        "sh",
        &[
            "-c".into(),
            "ls -1 ~/.config/margo/colorschemes 2>/dev/null".into(),
        ],
    );
    let mut presets: Vec<String> = out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    presets.sort();
    PRESETS.with(|p| *p.borrow_mut() = presets);
}

/// Write the current EDITS map to `~/.config/margo/colors.conf` + reload margo.
fn apply_current_edits() {
    let body = compose_conf(&EDITS.with(|e| e.borrow().clone()));
    let cmd = format!(
        "umask 022 && printf %s {} > ~/.config/margo/colors.conf && mctl reload --force",
        shell_quote(&body)
    );
    let out = host::run("sh", &["-c".into(), cmd]);
    STATUS.with(|s| {
        *s.borrow_mut() = if out.code == 0 {
            "Applied current edits to colors.conf.".into()
        } else {
            format!("Apply failed: {}", out.stderr.trim())
        };
    });
}

fn save_as_preset(name: &str) {
    let safe = sanitize_name(name);
    if safe.is_empty() {
        STATUS.with(|s| {
            *s.borrow_mut() = "Preset name can't be empty.".into();
        });
        return;
    }
    let body = compose_conf(&EDITS.with(|e| e.borrow().clone()));
    let cmd = format!(
        "umask 022 && mkdir -p ~/.config/margo/colorschemes/{safe} && printf %s {} > ~/.config/margo/colorschemes/{safe}/colors.conf",
        shell_quote(&body)
    );
    let out = host::run("sh", &["-c".into(), cmd]);
    if out.code == 0 {
        STATUS.with(|s| {
            *s.borrow_mut() = format!("Saved preset `{safe}`.");
        });
        NEW_NAME.with(|n| n.borrow_mut().clear());
        load_presets();
    } else {
        STATUS.with(|s| {
            *s.borrow_mut() = format!("Save failed: {}", out.stderr.trim());
        });
    }
}

fn apply_preset(name: &str) {
    let safe = sanitize_name(name);
    let cmd = format!(
        "cp ~/.config/margo/colorschemes/{safe}/colors.conf ~/.config/margo/colors.conf \
            && mctl reload --force"
    );
    let out = host::run("sh", &["-c".into(), cmd]);
    if out.code == 0 {
        STATUS.with(|s| {
            *s.borrow_mut() = format!("Applied preset `{safe}`. Wallpaper-driven theming will overwrite on next regen.");
        });
        load_current();
    } else {
        STATUS.with(|s| {
            *s.borrow_mut() = format!("Apply failed: {}", out.stderr.trim());
        });
    }
}

fn delete_preset(name: &str) {
    let safe = sanitize_name(name);
    if safe.is_empty() {
        return;
    }
    let cmd = format!("rm -rf ~/.config/margo/colorschemes/{safe}");
    let _ = host::run("sh", &["-c".into(), cmd]);
    STATUS.with(|s| *s.borrow_mut() = format!("Deleted preset `{safe}`."));
    load_presets();
}

fn compose_conf(edits: &BTreeMap<String, String>) -> String {
    let mut s = String::new();
    s.push_str("# Written by the color-scheme-creator plugin.\n");
    s.push_str("# `source`d from config.conf; overrides the static border colours.\n");
    for (key, _) in TOKENS {
        if let Some(v) = edits.get(*key) {
            s.push_str(&format!("{key:<15} = {v}\n"));
        }
    }
    s
}

/// Bash single-quote `s` so the printf in our shell-out doesn't break on
/// special characters (the colours.conf body is plain ascii but defence in
/// depth is cheap).
fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn sanitize_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

// ── Rendering ────────────────────────────────────────────────────────────

fn token_row(key: &str, label: &str, value: &str) -> El {
    El::hbox(vec![
        El::label(label).halign("start").prop("width", "180"),
        El::entry(format!("hex:{key}"), value)
            .class("plugin-search")
            .prop("width", "180"),
        El::label(value.to_string())
            .class("dim-label")
            .halign("end")
            .prop("width", "120"),
    ])
    .spacing(8)
    .padding(4)
    .class("plugin-bar-row")
}

fn preset_row(name: &str) -> El {
    El::hbox(vec![
        El::image("preferences-desktop-color-symbolic"),
        El::label(name.to_string()).halign("start").hexpand(true),
        El::button(format!("apply:{name}"), "Apply")
            .class("plugin-action plugin-action-primary"),
        El::button(format!("del:{name}"), "")
            .class("plugin-panel-action")
            .prop("icon", "user-trash-symbolic"),
    ])
    .spacing(8)
    .padding(4)
    .class("plugin-row")
}

fn view_tree() -> El {
    ensure_loaded();

    let edits = EDITS.with(|e| e.borrow().clone());
    let presets = PRESETS.with(|p| p.borrow().clone());
    let new_name = NEW_NAME.with(|n| n.borrow().clone());
    let status = STATUS.with(|s| s.borrow().clone());

    let header = El::hbox(vec![
        El::image("preferences-desktop-color-symbolic"),
        El::label("Color Scheme Creator")
            .hexpand(true)
            .halign("start"),
        El::button("refresh", "")
            .class("plugin-panel-action")
            .prop("icon", "view-refresh-symbolic"),
    ])
    .spacing(8)
    .class("plugin-panel-header");

    // ── Presets list ─────────────────────────────────────────────
    let mut preset_children: Vec<El> = vec![El::label("Saved presets")
        .class("label-medium-bold")
        .halign("start")];
    if presets.is_empty() {
        preset_children.push(
            El::label("No presets yet — save the current colours below.")
                .class("dim-label")
                .halign("start")
                .padding(4),
        );
    } else {
        for name in &presets {
            preset_children.push(preset_row(name));
        }
    }
    let presets_section = El::vbox(preset_children).spacing(4);

    // ── Editor ───────────────────────────────────────────────────
    let mut editor_children: Vec<El> = vec![El::label("Current colors.conf")
        .class("label-medium-bold")
        .halign("start")];
    for (key, label) in TOKENS {
        let v = edits.get(*key).cloned().unwrap_or_default();
        editor_children.push(token_row(key, label, &v));
    }
    editor_children.push(
        El::hbox(vec![
            El::button("apply-edits", "Apply edits")
                .class("plugin-action plugin-action-primary plugin-expand"),
            El::button("reload-current", "Reload from disk")
                .class("plugin-action plugin-expand"),
        ])
        .spacing(8),
    );
    let editor_section = El::vbox(editor_children).spacing(4).class("plugin-card");

    // ── Save current as ──────────────────────────────────────────
    let save_section = El::vbox(vec![
        El::label("Save current as…")
            .class("label-medium-bold")
            .halign("start"),
        El::hbox(vec![
            El::entry("save-name", &new_name)
                .class("plugin-search")
                .hexpand(true),
            El::button("save", "Save")
                .class("plugin-action plugin-action-primary"),
        ])
        .spacing(8),
    ])
    .spacing(4);

    let status_label = if status.is_empty() {
        El::label("Caveat: matugen regenerates colors.conf on wallpaper change. Manual edits last until the next regen.")
            .class("dim-label")
            .halign("center")
            .padding(8)
    } else {
        El::label(status).class("dim-label").halign("center").padding(8)
    };

    El::vbox(vec![
        header,
        presets_section,
        El::separator(),
        editor_section,
        El::separator(),
        save_section,
        status_label,
    ])
    .spacing(14)
    .class("plugin-panel-large")
}

struct ColorSchemeCreator;

impl Component for ColorSchemeCreator {
    fn view() -> El {
        view_tree()
    }
    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(rest) = id.strip_prefix("apply:") {
                    apply_preset(rest);
                } else if let Some(rest) = id.strip_prefix("del:") {
                    delete_preset(rest);
                } else {
                    match id {
                        "refresh" | "reload-current" => {
                            load_current();
                            load_presets();
                            STATUS.with(|s| s.borrow_mut().clear());
                        }
                        "apply-edits" => apply_current_edits(),
                        "save" => {
                            let name = NEW_NAME.with(|n| n.borrow().clone());
                            save_as_preset(&name);
                        }
                        _ => {}
                    }
                }
            }
            EventKind::Submit => {
                let id = ev.id.as_str();
                if let Some(token) = id.strip_prefix("hex:") {
                    EDITS.with(|e| {
                        e.borrow_mut().insert(token.into(), ev.value.trim().to_string());
                    });
                } else if id == "save-name" {
                    NEW_NAME.with(|n| *n.borrow_mut() = ev.value.clone());
                    if !ev.value.trim().is_empty() {
                        save_as_preset(&ev.value);
                    }
                }
            }
            EventKind::Keybind if ev.id == "open" => {
                load_current();
                load_presets();
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(ColorSchemeCreator);
