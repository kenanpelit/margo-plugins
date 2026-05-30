//! Clipper — a clipboard manager for the margo shell, written with the margo
//! plugin authoring SDK (`mplugin-sdk`).
//!
//! A best-of-both port of noctalia's `clipper` and DMS's `ClipboardPlus`,
//! built on the same backend they use — [`cliphist`](https://github.com/sentriz/cliphist)
//! + wl-clipboard. Searchable history (text + images), one-click copy,
//! per-entry delete, clear-all, optional auto-paste, and **pinned items** that
//! survive history rotation (stored in the plugin's data dir).
//!
//! All clipboard I/O goes through the host's `run` capability:
//! - list:   `cliphist list`               → `<id>\t<preview>` lines
//! - copy:   `cliphist decode <id> | wl-copy`  (text *and* images)
//! - delete: `cliphist list | awk '$1==id' | cliphist delete`
//! - wipe:   `cliphist wipe`
//! - paste:  `wtype -M ctrl v -m ctrl`      (optional, best-effort)

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

const PINS_FILE: &str = "pins.json";

#[derive(Clone)]
struct Entry {
    id: String,
    preview: String,
    is_image: bool,
}

#[derive(Clone)]
struct Pin {
    label: String,
    content: String,
}

thread_local! {
    static ITEMS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static PINS: RefCell<Vec<Pin>> = const { RefCell::new(Vec::new()) };
    static SEARCH: RefCell<String> = const { RefCell::new(String::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Settings ──────────────────────────────────────────────────────────────

fn max_items() -> usize {
    host::get_setting("max_items").trim().parse().unwrap_or(60).clamp(1, 500)
}

fn auto_paste() -> bool {
    matches!(host::get_setting("auto_paste").to_lowercase().as_str(), "yes" | "true" | "1")
}

// ── cliphist plumbing ───────────────────────────────────────────────────────

fn sh(cmd: String) -> host::ProcessOutput {
    host::run("sh", &["-c".to_string(), cmd])
}

/// A cliphist id is always a decimal integer — validate before interpolating
/// it into a shell command (the preview text is never interpolated).
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

fn looks_like_image(preview: &str) -> bool {
    let p = preview.to_lowercase();
    p.contains("binary data")
        && ["png", "jpg", "jpeg", "gif", "bmp", "webp", "image"]
            .iter()
            .any(|k| p.contains(k))
}

fn load_items() {
    let out = host::run("cliphist", &["list".to_string()]);
    let items: Vec<Entry> = out
        .stdout
        .lines()
        .filter_map(|line| {
            let (id, preview) = line.split_once('\t')?;
            let id = id.trim();
            if !valid_id(id) {
                return None;
            }
            Some(Entry {
                id: id.to_string(),
                preview: preview.trim().to_string(),
                is_image: looks_like_image(preview),
            })
        })
        .collect();
    ITEMS.with(|i| *i.borrow_mut() = items);
}

fn refresh() {
    load_items();
}

fn ensure_loaded() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    load_pins();
    load_items();
}

fn copy_id(id: &str) {
    if !valid_id(id) {
        return;
    }
    sh(format!("cliphist decode {id} | wl-copy"));
    after_copy();
}

fn copy_text(text: &str) {
    host::copy(text);
    after_copy();
}

fn after_copy() {
    if auto_paste() {
        // Best-effort: a short delay lets the panel close and focus return to
        // the target window before we replay Ctrl+V.
        sh("sleep 0.18; wtype -M ctrl v -m ctrl".to_string());
    } else {
        host::notify("Clipboard", "Copied to clipboard");
    }
}

fn delete_id(id: &str) {
    if !valid_id(id) {
        return;
    }
    // Match the row whose first tab-field equals the id, then pipe that exact
    // line to `cliphist delete` (so the arbitrary preview is never quoted).
    sh(format!(
        "cliphist list | awk -F'\\t' -v id={id} '$1==id' | cliphist delete"
    ));
    refresh();
}

fn wipe() {
    host::run("cliphist", &["wipe".to_string()]);
    refresh();
}

fn decode_text(id: &str) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    let out = host::run("cliphist", &["decode".to_string(), id.to_string()]);
    if out.code == 0 && !out.stdout.is_empty() {
        Some(out.stdout)
    } else {
        None
    }
}

// ── Pins (persisted) ─────────────────────────────────────────────────────────

fn load_pins() {
    let Ok(bytes) = host::read_file(PINS_FILE) else {
        return;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    if let Some(arr) = v.as_array() {
        let pins: Vec<Pin> = arr
            .iter()
            .filter_map(|p| {
                Some(Pin {
                    label: p["label"].as_str()?.to_string(),
                    content: p["content"].as_str()?.to_string(),
                })
            })
            .collect();
        PINS.with(|x| *x.borrow_mut() = pins);
    }
}

fn save_pins() {
    let arr: Vec<serde_json::Value> = PINS.with(|p| {
        p.borrow()
            .iter()
            .map(|pin| serde_json::json!({ "label": pin.label, "content": pin.content }))
            .collect()
    });
    let _ = host::write_file(PINS_FILE, &serde_json::Value::Array(arr).to_string().into_bytes());
}

/// Pin a text history entry: decode its full content now so it survives
/// cliphist rotation. (Images aren't pinnable — they can't round-trip as text.)
fn pin_id(id: &str, preview: &str) {
    let Some(content) = decode_text(id) else {
        host::notify("Clipboard", "Couldn't pin this entry.");
        return;
    };
    let label = preview.chars().take(80).collect::<String>();
    PINS.with(|p| {
        // De-dupe by content.
        if !p.borrow().iter().any(|x| x.content == content) {
            p.borrow_mut().push(Pin { label, content });
        }
    });
    save_pins();
}

fn unpin(index: usize) {
    PINS.with(|p| {
        if index < p.borrow().len() {
            p.borrow_mut().remove(index);
        }
    });
    save_pins();
}

// ── UI ──────────────────────────────────────────────────────────────────────

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn header() -> El {
    let count = ITEMS.with(|i| i.borrow().len());
    El::hbox(vec![
        El::image("edit-paste-symbolic"),
        El::vbox(vec![
            El::label("Clipboard").class("label-large-bold").halign("start"),
            El::label(format!("{count} items")).class("dim-label").halign("start"),
        ])
        .spacing(2)
        .hexpand(true),
        El::button("refresh", "")
            .prop("icon", "view-refresh-symbolic")
            .class("plugin-panel-action"),
        El::button("wipe", "")
            .prop("icon", "edit-delete-symbolic")
            .class("plugin-panel-action"),
    ])
    .class("plugin-panel-header")
    .spacing(8)
}

fn pin_row(i: usize, pin: &Pin) -> El {
    El::hbox(vec![
        El::button(format!("pcopy:{i}"), truncate(&pin.label, 64))
            .prop("icon", "starred-symbolic")
            .class("plugin-row")
            .hexpand(true)
            .halign("fill"),
        El::button(format!("unpin:{i}"), "")
            .prop("icon", "user-trash-symbolic")
            .class("plugin-panel-action"),
    ])
    .spacing(6)
}

fn history_row(e: &Entry) -> El {
    let (icon, label) = if e.is_image {
        ("image-x-generic-symbolic", truncate(&e.preview, 64))
    } else {
        ("text-x-generic-symbolic", truncate(&e.preview, 80))
    };
    let mut row = vec![
        El::button(format!("copy:{}", e.id), label)
            .prop("icon", icon)
            .class("plugin-row")
            .hexpand(true)
            .halign("fill"),
    ];
    // Only text entries can be pinned (images can't round-trip as text).
    if !e.is_image {
        row.push(
            El::button(format!("pin:{}|{}", e.id, truncate(&e.preview, 80)), "")
                .prop("icon", "non-starred-symbolic")
                .class("plugin-panel-action"),
        );
    }
    row.push(
        El::button(format!("del:{}", e.id), "")
            .prop("icon", "user-trash-symbolic")
            .class("plugin-panel-action"),
    );
    El::hbox(row).spacing(6)
}

fn view_tree() -> El {
    ensure_loaded();
    let needle = SEARCH.with(|s| s.borrow().to_lowercase());
    let pins = PINS.with(|p| p.borrow().clone());
    let items = ITEMS.with(|i| i.borrow().clone());

    let filtered: Vec<Entry> = items
        .into_iter()
        .filter(|e| needle.is_empty() || e.preview.to_lowercase().contains(&needle))
        .take(max_items())
        .collect();

    let mut rows: Vec<El> = Vec::new();

    // Pinned section (filtered by search too).
    let pin_matches: Vec<(usize, Pin)> = pins
        .iter()
        .enumerate()
        .filter(|(_, p)| needle.is_empty() || p.label.to_lowercase().contains(&needle))
        .map(|(i, p)| (i, p.clone()))
        .collect();
    if !pin_matches.is_empty() {
        rows.push(El::label("Pinned").class("plugin-section-title").halign("start"));
        for (i, p) in &pin_matches {
            rows.push(pin_row(*i, p));
        }
        rows.push(El::separator());
    }

    if filtered.is_empty() && pin_matches.is_empty() {
        rows.push(
            El::label(if needle.is_empty() {
                "Clipboard history is empty."
            } else {
                "No matching clips."
            })
            .class("dim-label")
            .halign("center")
            .padding(16),
        );
    } else {
        rows.push(El::label("History").class("plugin-section-title").halign("start"));
        for e in &filtered {
            rows.push(history_row(e));
        }
    }

    El::vbox(vec![
        header(),
        El::entry("search", &SEARCH.with(|s| s.borrow().clone()))
            .class("plugin-search")
            .prop("placeholder", "Search clipboard…")
            .hexpand(true),
        El::scroll(rows).class("plugin-list").vexpand(true).spacing(4),
    ])
    .spacing(12)
    .class("plugin-panel-large")
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Clipper;

impl Component for Clipper {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            // Live-filter as the user types.
            EventKind::Input | EventKind::Submit if ev.id == "search" => {
                SEARCH.with(|s| *s.borrow_mut() = ev.value.clone());
            }
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(cid) = id.strip_prefix("copy:") {
                    copy_id(cid);
                } else if let Some(rest) = id.strip_prefix("pin:") {
                    // payload is "<id>|<preview>"
                    if let Some((cid, preview)) = rest.split_once('|') {
                        pin_id(cid, preview);
                    }
                } else if let Some(cid) = id.strip_prefix("del:") {
                    delete_id(cid);
                } else if let Some(rest) = id.strip_prefix("pcopy:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        let content = PINS.with(|p| p.borrow().get(i).map(|x| x.content.clone()));
                        if let Some(c) = content {
                            copy_text(&c);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("unpin:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        unpin(i);
                    }
                } else {
                    match id {
                        "refresh" => refresh(),
                        "wipe" => wipe(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Clipper);
