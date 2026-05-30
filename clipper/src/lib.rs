//! Clipper — a clipboard manager + sticky notes for the margo shell, written
//! with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A best-of-both port of noctalia's `clipper` and DMS's `ClipboardPlus`,
//! built on the same backend (cliphist + wl-clipboard). Three tabs:
//!   • Clipboard — searchable history (text + image thumbnails), copy / pin /
//!     delete, clear-all.
//!   • Pinned    — text clips you kept; survive cliphist rotation.
//!   • Notes     — colour-coded sticky notes (create / edit / recolour / copy /
//!     delete), persisted in the plugin's data dir.
//!
//! Clipboard I/O via the host `run` cap; cliphist ids are validated numeric
//! before any shell interpolation, and entry previews are never interpolated.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;
use std::collections::HashSet;

const PINS_FILE: &str = "pins.json";
const NOTES_FILE: &str = "notes.json";
const THUMB_DIR: &str = "/tmp/margo-clipper";

/// Sticky-note background palette (name, bg hex) — mirrors clipper's schemes.
const NOTE_COLORS: &[(&str, &str)] = &[
    ("yellow", "#FFF9C4"),
    ("pink", "#FCE4EC"),
    ("blue", "#E3F2FD"),
    ("green", "#E8F5E9"),
    ("purple", "#F3E5F5"),
];

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Clipboard,
    Pinned,
    Notes,
}

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

#[derive(Clone)]
struct Note {
    id: u64,
    text: String,
    color: usize,
}

thread_local! {
    static TAB: RefCell<Tab> = const { RefCell::new(Tab::Clipboard) };
    static ITEMS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static PINS: RefCell<Vec<Pin>> = const { RefCell::new(Vec::new()) };
    static NOTES: RefCell<Vec<Note>> = const { RefCell::new(Vec::new()) };
    static NOTE_NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
    static NOTE_DRAFT: RefCell<String> = const { RefCell::new(String::new()) };
    static EDITING_NOTE: RefCell<Option<u64>> = const { RefCell::new(None) };
    static SEARCH: RefCell<String> = const { RefCell::new(String::new()) };
    /// cliphist ids whose image thumbnail we've already decoded this session.
    static DECODED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
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

/// Content type of a clip — drives the card's tint, icon, and label (mirrors
/// clipper's Text / Image / Colour / Link / Code / File detection).
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Image,
    Color,
    Link,
    Code,
    File,
}

fn is_hex_color(t: &str) -> bool {
    let h = t.strip_prefix('#').unwrap_or("");
    matches!(h.len(), 3 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
}

fn looks_code(t: &str) -> bool {
    let starts = t.starts_with(['{', '[', '(', '<']);
    let kw = [
        "function", "import ", "const ", "let ", "var ", "class ", "def ", "return ",
        "#include", "fn ", "=>",
    ];
    starts || kw.iter().any(|k| t.contains(k))
}

fn detect_kind(e: &Entry) -> Kind {
    if e.is_image {
        return Kind::Image;
    }
    let t = e.preview.trim();
    if is_hex_color(t) {
        Kind::Color
    } else if t.starts_with("http://") || t.starts_with("https://") {
        Kind::Link
    } else if t.starts_with('/') || t.starts_with('~') || t.starts_with("file://") {
        Kind::File
    } else if looks_code(t) {
        Kind::Code
    } else {
        Kind::Text
    }
}

/// (css-class suffix, type label, type icon) for a kind.
fn kind_meta(k: Kind) -> (&'static str, &'static str, &'static str) {
    match k {
        Kind::Text => ("text", "Text", "text-x-generic-symbolic"),
        Kind::Image => ("image", "Image", "image-x-generic-symbolic"),
        Kind::Color => ("color", "Colour", "color-select-symbolic"),
        Kind::Link => ("link", "Link", "web-browser-symbolic"),
        Kind::Code => ("code", "Code", "utilities-terminal-symbolic"),
        Kind::File => ("file", "File", "folder-symbolic"),
    }
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

/// Decode an image entry to a stable temp file once (so the row can show a
/// real thumbnail), returning its path. Cached per session by id.
fn thumb_path(id: &str) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    let path = format!("{THUMB_DIR}/{id}");
    let already = DECODED.with(|d| d.borrow().contains(id));
    if !already {
        // `[ -s ]` guard makes it idempotent across shell restarts too.
        sh(format!(
            "mkdir -p {THUMB_DIR}; [ -s '{path}' ] || cliphist decode {id} > '{path}' 2>/dev/null"
        ));
        DECODED.with(|d| {
            d.borrow_mut().insert(id.to_string());
        });
    }
    Some(path)
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
        sh("sleep 0.18; wtype -M ctrl v -m ctrl".to_string());
    } else {
        host::notify("Clipboard", "Copied to clipboard");
    }
}

fn delete_id(id: &str) {
    if !valid_id(id) {
        return;
    }
    sh(format!(
        "cliphist list | awk -F'\\t' -v id={id} '$1==id' | cliphist delete"
    ));
    load_items();
}

fn wipe() {
    host::run("cliphist", &["wipe".to_string()]);
    load_items();
}

fn decode_text(id: &str) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    let out = host::run("cliphist", &["decode".to_string(), id.to_string()]);
    (out.code == 0 && !out.stdout.is_empty()).then_some(out.stdout)
}

// ── Persistence ──────────────────────────────────────────────────────────────

fn ensure_loaded() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    load_pins();
    load_notes();
    load_items();
}

fn load_pins() {
    if let Ok(bytes) = host::read_file(PINS_FILE) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(arr) = v.as_array() {
                let pins = arr
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

fn load_notes() {
    if let Ok(bytes) = host::read_file(NOTES_FILE) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(arr) = v.as_array() {
                let notes: Vec<Note> = arr
                    .iter()
                    .filter_map(|n| {
                        Some(Note {
                            id: n["id"].as_u64()?,
                            text: n["text"].as_str()?.to_string(),
                            color: n["color"].as_u64().unwrap_or(0) as usize % NOTE_COLORS.len(),
                        })
                    })
                    .collect();
                let next = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                NOTE_NEXT_ID.with(|x| *x.borrow_mut() = next);
                NOTES.with(|x| *x.borrow_mut() = notes);
            }
        }
    }
}

fn save_notes() {
    let arr: Vec<serde_json::Value> = NOTES.with(|n| {
        n.borrow()
            .iter()
            .map(|note| serde_json::json!({ "id": note.id, "text": note.text, "color": note.color }))
            .collect()
    });
    let _ = host::write_file(NOTES_FILE, &serde_json::Value::Array(arr).to_string().into_bytes());
}

fn pin_id(id: &str, preview: &str) {
    let Some(content) = decode_text(id) else {
        host::notify("Clipboard", "Couldn't pin this entry.");
        return;
    };
    let label = preview.chars().take(80).collect::<String>();
    PINS.with(|p| {
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

// ── Notes actions ────────────────────────────────────────────────────────────

fn commit_note() {
    let text = NOTE_DRAFT.with(|d| d.borrow().trim().to_string());
    if text.is_empty() {
        return;
    }
    if let Some(eid) = EDITING_NOTE.with(|e| *e.borrow()) {
        NOTES.with(|n| {
            if let Some(note) = n.borrow_mut().iter_mut().find(|x| x.id == eid) {
                note.text = text;
            }
        });
    } else {
        let id = NOTE_NEXT_ID.with(|x| {
            let v = *x.borrow();
            *x.borrow_mut() = v + 1;
            v
        });
        NOTES.with(|n| n.borrow_mut().push(Note { id, text, color: 0 }));
    }
    NOTE_DRAFT.with(|d| d.borrow_mut().clear());
    EDITING_NOTE.with(|e| *e.borrow_mut() = None);
    save_notes();
}

fn edit_note(id: u64) {
    let text = NOTES.with(|n| n.borrow().iter().find(|x| x.id == id).map(|x| x.text.clone()));
    if let Some(t) = text {
        EDITING_NOTE.with(|e| *e.borrow_mut() = Some(id));
        NOTE_DRAFT.with(|d| *d.borrow_mut() = t);
    }
}

fn cycle_note_color(id: u64) {
    NOTES.with(|n| {
        if let Some(note) = n.borrow_mut().iter_mut().find(|x| x.id == id) {
            note.color = (note.color + 1) % NOTE_COLORS.len();
        }
    });
    save_notes();
}

fn delete_note(id: u64) {
    NOTES.with(|n| n.borrow_mut().retain(|x| x.id != id));
    if EDITING_NOTE.with(|e| *e.borrow()) == Some(id) {
        EDITING_NOTE.with(|e| *e.borrow_mut() = None);
        NOTE_DRAFT.with(|d| d.borrow_mut().clear());
    }
    save_notes();
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn tabs() -> El {
    let cur = TAB.with(|t| *t.borrow());
    let chip = |id: &str, label: &str, t: Tab| {
        let class = if cur == t {
            "plugin-chip plugin-chip-on"
        } else {
            "plugin-chip"
        };
        El::button(id.to_string(), label).class(class)
    };
    El::hbox(vec![
        chip("tab:clip", "Clipboard", Tab::Clipboard),
        chip("tab:pin", "Pinned", Tab::Pinned),
        chip("tab:note", "Notes", Tab::Notes),
    ])
    .spacing(6)
    .halign("center")
}

// ── Tab: Clipboard ──────────────────────────────────────────────────────────

/// One clipboard entry as a type-tinted card (clipper's board look).
fn clip_card(e: &Entry) -> El {
    let kind = detect_kind(e);
    let (suffix, label, icon) = kind_meta(kind);

    // Header: type icon + label, then pin (text-only) + delete.
    let mut head = vec![
        El::image(icon).valign("center"),
        El::label(label).class("clip-card-type").halign("start").hexpand(true),
    ];
    if !e.is_image {
        head.push(
            El::button(format!("pin:{}|{}", e.id, truncate(&e.preview, 80)), "")
                .prop("icon", "non-starred-symbolic")
                .class("plugin-panel-action"),
        );
    }
    head.push(
        El::button(format!("del:{}", e.id), "")
            .prop("icon", "user-trash-symbolic")
            .class("plugin-panel-action"),
    );

    // Body: thumbnail for images, a colour swatch for colours, else excerpt.
    let body: El = if e.is_image {
        let thumb = thumb_path(&e.id)
            .map(|p| {
                El::image(p)
                    .prop("fit", "cover")
                    .prop("width", "150")
                    .prop("height", "90")
                    .halign("center")
            })
            .unwrap_or_else(|| El::label("image").class("dim-label"));
        El::vbox(vec![
            thumb,
            El::button(format!("copy:{}", e.id), "Copy image").class("plugin-action plugin-expand"),
        ])
        .spacing(6)
    } else if matches!(kind, Kind::Color) {
        El::hbox(vec![
            El::color_swatch(e.preview.trim(), 36).valign("center"),
            El::button(format!("copy:{}", e.id), e.preview.trim().to_string())
                .class("clip-card-body")
                .hexpand(true)
                .halign("fill"),
        ])
        .spacing(8)
    } else {
        El::button(format!("copy:{}", e.id), truncate(&e.preview, 140))
            .class("clip-card-body")
            .hexpand(true)
            .halign("fill")
    };

    El::vbox(vec![El::hbox(head).spacing(4), body])
        .spacing(6)
        .class(format!("clip-card clip-card-{suffix}"))
}

fn clipboard_tab() -> El {
    let needle = SEARCH.with(|s| s.borrow().to_lowercase());
    let items = ITEMS.with(|i| i.borrow().clone());
    let filtered: Vec<Entry> = items
        .into_iter()
        .filter(|e| needle.is_empty() || e.preview.to_lowercase().contains(&needle))
        .take(max_items())
        .collect();

    let search = El::entry("search", &SEARCH.with(|s| s.borrow().clone()))
        .class("plugin-search")
        .prop("placeholder", "Search clipboard… (Enter)")
        .hexpand(true);

    let board: El = if filtered.is_empty() {
        El::label(if needle.is_empty() {
            "Clipboard history is empty."
        } else {
            "No matching clips."
        })
        .class("dim-label")
        .halign("center")
        .padding(16)
    } else {
        // A board of type-tinted cards, two columns.
        let cards: Vec<El> = filtered.iter().map(clip_card).collect();
        El::grid(2, cards).spacing(8)
    };

    El::vbox(vec![search, El::scroll(vec![board]).vexpand(true)]).spacing(8)
}

// ── Tab: Pinned ───────────────────────────────────────────────────────────────

fn pinned_tab() -> El {
    let pins = PINS.with(|p| p.borrow().clone());
    let mut rows: Vec<El> = Vec::new();
    {
        for (i, p) in pins.iter().enumerate() {
            rows.push(
                El::vbox(vec![
                    El::hbox(vec![
                        El::image("starred-symbolic").valign("center"),
                        El::label("Pinned").class("clip-card-type").halign("start").hexpand(true),
                        El::button(format!("unpin:{i}"), "")
                            .prop("icon", "user-trash-symbolic")
                            .class("plugin-panel-action"),
                    ])
                    .spacing(4),
                    El::button(format!("pcopy:{i}"), truncate(&p.label, 140))
                        .class("clip-card-body")
                        .hexpand(true)
                        .halign("fill"),
                ])
                .spacing(6)
                .class("clip-card clip-card-text"),
            );
        }
    }
    let board = if rows.is_empty() {
        El::label("No pinned clips. Pin a text clip with ☆ in the Clipboard tab.")
            .class("dim-label")
            .halign("center")
            .padding(16)
    } else {
        El::grid(2, rows).spacing(8)
    };
    El::scroll(vec![board]).vexpand(true)
}

// ── Tab: Notes ────────────────────────────────────────────────────────────────

fn note_card(note: &Note) -> El {
    let (cname, _chex) = NOTE_COLORS[note.color];
    El::vbox(vec![
        // The note text fills the coloured card; click to copy.
        El::button(format!("ncopy:{}", note.id), truncate(&note.text, 160))
            .class("clip-card-body")
            .hexpand(true)
            .halign("fill"),
        // Footer actions: recolour · edit · delete.
        El::hbox(vec![
            El::label("").hexpand(true),
            El::button(format!("ncolor:{}", note.id), "")
                .prop("icon", "color-select-symbolic")
                .class("plugin-panel-action"),
            El::button(format!("nedit:{}", note.id), "")
                .prop("icon", "document-edit-symbolic")
                .class("plugin-panel-action"),
            El::button(format!("ndelete:{}", note.id), "")
                .prop("icon", "user-trash-symbolic")
                .class("plugin-panel-action"),
        ])
        .spacing(2),
    ])
    .spacing(4)
    .class(format!("note-card note-card-{cname}"))
}

fn notes_tab() -> El {
    let notes = NOTES.with(|n| n.borrow().clone());
    let draft = NOTE_DRAFT.with(|d| d.borrow().clone());
    let editing = EDITING_NOTE.with(|e| *e.borrow()).is_some();

    let mut children: Vec<El> = Vec::new();
    if editing {
        children.push(
            El::hbox(vec![
                El::label("Editing note").class("dim-label").halign("start").hexpand(true),
                El::button("ncancel", "")
                    .prop("icon", "edit-clear-symbolic")
                    .class("plugin-panel-action"),
            ])
            .spacing(6),
        );
    }
    children.push(
        El::hbox(vec![
            El::entry("note", &draft)
                .class("plugin-search")
                .prop("placeholder", "New note… (Enter to save)")
                .hexpand(true),
            El::button("nadd", "")
                .prop("icon", if editing { "object-select-symbolic" } else { "list-add-symbolic" })
                .class("plugin-action plugin-action-primary"),
        ])
        .spacing(6),
    );

    let board: El = if notes.is_empty() {
        El::label("No notes yet — jot one above.")
            .class("dim-label")
            .halign("center")
            .padding(16)
    } else {
        let cards: Vec<El> = notes.iter().map(note_card).collect();
        El::grid(2, cards).spacing(8)
    };
    children.push(El::scroll(vec![board]).vexpand(true));
    El::vbox(children).spacing(8)
}

// ── Root view ────────────────────────────────────────────────────────────────

fn header() -> El {
    let tab = TAB.with(|t| *t.borrow());
    let mut right: Vec<El> = vec![El::button("refresh", "")
        .prop("icon", "view-refresh-symbolic")
        .class("plugin-panel-action")];
    if tab == Tab::Clipboard {
        right.push(
            El::button("wipe", "")
                .prop("icon", "edit-delete-symbolic")
                .class("plugin-panel-action"),
        );
    }
    El::hbox(vec![
        El::image("edit-paste-symbolic"),
        El::label("Clipper").class("label-large-bold").halign("start").hexpand(true),
        El::hbox(right).spacing(4),
    ])
    .class("plugin-panel-header")
    .spacing(8)
}

fn view_tree() -> El {
    ensure_loaded();
    let body = match TAB.with(|t| *t.borrow()) {
        Tab::Clipboard => clipboard_tab(),
        Tab::Pinned => pinned_tab(),
        Tab::Notes => notes_tab(),
    };
    El::vbox(vec![header(), tabs(), El::separator(), body])
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
            EventKind::Submit if ev.id == "search" => {
                SEARCH.with(|s| *s.borrow_mut() = ev.value.clone());
            }
            EventKind::Submit if ev.id == "note" => {
                NOTE_DRAFT.with(|d| *d.borrow_mut() = ev.value.clone());
                commit_note();
            }
            EventKind::Input if ev.id == "note" => {
                NOTE_DRAFT.with(|d| *d.borrow_mut() = ev.value.clone());
            }
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(cid) = id.strip_prefix("copy:") {
                    copy_id(cid);
                } else if let Some(rest) = id.strip_prefix("pin:") {
                    if let Some((cid, preview)) = rest.split_once('|') {
                        pin_id(cid, preview);
                    }
                } else if let Some(cid) = id.strip_prefix("del:") {
                    delete_id(cid);
                } else if let Some(rest) = id.strip_prefix("pcopy:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        if let Some(c) = PINS.with(|p| p.borrow().get(i).map(|x| x.content.clone())) {
                            copy_text(&c);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("unpin:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        unpin(i);
                    }
                } else if let Some(rest) = id.strip_prefix("ncopy:") {
                    if let Ok(nid) = rest.parse::<u64>() {
                        if let Some(t) = NOTES.with(|n| n.borrow().iter().find(|x| x.id == nid).map(|x| x.text.clone())) {
                            copy_text(&t);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("nedit:") {
                    if let Ok(nid) = rest.parse::<u64>() {
                        edit_note(nid);
                    }
                } else if let Some(rest) = id.strip_prefix("ncolor:") {
                    if let Ok(nid) = rest.parse::<u64>() {
                        cycle_note_color(nid);
                    }
                } else if let Some(rest) = id.strip_prefix("ndelete:") {
                    if let Ok(nid) = rest.parse::<u64>() {
                        delete_note(nid);
                    }
                } else {
                    match id {
                        "tab:clip" => TAB.with(|t| *t.borrow_mut() = Tab::Clipboard),
                        "tab:pin" => TAB.with(|t| *t.borrow_mut() = Tab::Pinned),
                        "tab:note" => TAB.with(|t| *t.borrow_mut() = Tab::Notes),
                        "refresh" => load_items(),
                        "wipe" => wipe(),
                        "nadd" => commit_note(),
                        "ncancel" => {
                            EDITING_NOTE.with(|e| *e.borrow_mut() = None);
                            NOTE_DRAFT.with(|d| d.borrow_mut().clear());
                        }
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
