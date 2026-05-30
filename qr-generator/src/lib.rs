//! QR Generator — make a QR code from text, the clipboard, or the current
//! Wi-Fi network, written with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A port of Dank Material Shell's `dms-qr-generator`, scoped to the WASM tier:
//! type (or paste, or grab Wi-Fi) → `qrencode` writes a PNG → the panel shows
//! it via an `image` node. The encoded text is copyable, and the PNG can be
//! put straight on the clipboard.
//!
//! The text is passed to qrencode as a single argv element after `--`, so no
//! shell quoting is involved and arbitrary content is safe.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

const QR_DIR: &str = "/tmp/margo-qr";

thread_local! {
    /// The text currently encoded (empty = nothing generated yet).
    static TEXT: RefCell<String> = const { RefCell::new(String::new()) };
    /// Path of the rendered PNG (changes each generation so the image reloads).
    static QR_PATH: RefCell<String> = const { RefCell::new(String::new()) };
    /// Draft in the input box.
    static DRAFT: RefCell<String> = const { RefCell::new(String::new()) };
    /// Monotonic counter for unique PNG filenames.
    static SEQ: RefCell<u64> = const { RefCell::new(0) };
    /// Last error (e.g. qrencode missing / Wi-Fi unavailable).
    static ERROR: RefCell<String> = const { RefCell::new(String::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

fn scale() -> String {
    let s: u32 = host::get_setting("scale").trim().parse().unwrap_or(10);
    s.clamp(2, 30).to_string()
}

fn ensure_loaded() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    let _ = host::run("mkdir", &["-p".to_string(), QR_DIR.to_string()]);
}

/// Encode `text` to a fresh PNG and make it the current QR.
fn generate(text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    ERROR.with(|e| e.borrow_mut().clear());
    let n = SEQ.with(|s| {
        let v = *s.borrow() + 1;
        *s.borrow_mut() = v;
        v
    });
    let path = format!("{QR_DIR}/qr-{n}.png");
    // Direct exec (no shell): the text is one argv element after `--`.
    let out = host::run(
        "qrencode",
        &[
            "-o".to_string(),
            path.clone(),
            "-s".to_string(),
            scale(),
            "-m".to_string(),
            "2".to_string(),
            "--".to_string(),
            text.to_string(),
        ],
    );
    if out.code != 0 {
        ERROR.with(|e| {
            *e.borrow_mut() = if out.code == -1 {
                "qrencode not found — install it (e.g. `sudo pacman -S qrencode`).".to_string()
            } else {
                format!("qrencode failed: {}", out.stderr.trim())
            }
        });
        return;
    }
    TEXT.with(|t| *t.borrow_mut() = text.to_string());
    QR_PATH.with(|p| *p.borrow_mut() = path);
}

fn from_clipboard() {
    let text = host::clipboard_read();
    if text.trim().is_empty() {
        ERROR.with(|e| *e.borrow_mut() = "Clipboard is empty (or not text).".to_string());
    } else {
        DRAFT.with(|d| *d.borrow_mut() = text.clone());
        generate(&text);
    }
}

/// Build a `WIFI:` payload from the active Wi-Fi connection and encode it.
fn from_wifi() {
    let ssid = host::run(
        "sh",
        &[
            "-c".to_string(),
            "nmcli -t -f active,ssid dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1"
                .to_string(),
        ],
    )
    .stdout
    .trim()
    .to_string();
    if ssid.is_empty() {
        ERROR.with(|e| *e.borrow_mut() = "No active Wi-Fi network found.".to_string());
        return;
    }
    // -s reveals the secret; the profile name is usually the SSID.
    let psk = host::run(
        "nmcli",
        &[
            "-s".to_string(),
            "-g".to_string(),
            "802-11-wireless-security.psk".to_string(),
            "connection".to_string(),
            "show".to_string(),
            ssid.clone(),
        ],
    )
    .stdout
    .trim()
    .to_string();
    // WIFI: format — escape ; , : \ per the spec.
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,")
            .replace(':', "\\:")
    };
    let payload = if psk.is_empty() {
        format!("WIFI:T:nopass;S:{};;", esc(&ssid))
    } else {
        format!("WIFI:T:WPA;S:{};P:{};;", esc(&ssid), esc(&psk))
    };
    DRAFT.with(|d| *d.borrow_mut() = format!("Wi-Fi · {ssid}"));
    generate(&payload);
}

fn copy_image() {
    let path = QR_PATH.with(|p| p.borrow().clone());
    if !path.is_empty() {
        // path is /tmp/margo-qr/qr-<n>.png — no untrusted content in it.
        host::run("sh", &["-c".to_string(), format!("wl-copy -t image/png < '{path}'")]);
        host::notify("QR Generator", "QR image copied to clipboard");
    }
}

fn clear() {
    TEXT.with(|t| t.borrow_mut().clear());
    QR_PATH.with(|p| p.borrow_mut().clear());
    DRAFT.with(|d| d.borrow_mut().clear());
    ERROR.with(|e| e.borrow_mut().clear());
}

// ── UI ─────────────────────────────────────────────────────────────────────

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn view_tree() -> El {
    ensure_loaded();
    let draft = DRAFT.with(|d| d.borrow().clone());
    let qr = QR_PATH.with(|p| p.borrow().clone());
    let text = TEXT.with(|t| t.borrow().clone());
    let error = ERROR.with(|e| e.borrow().clone());

    let header = El::hbox(vec![
        El::image("scanner-symbolic"),
        El::label("QR Generator").class("label-large-bold").halign("start").hexpand(true),
        El::button("clear", "")
            .prop("icon", "edit-clear-symbolic")
            .class("plugin-panel-action"),
    ])
    .class("plugin-panel-header")
    .spacing(8);

    let input = El::hbox(vec![
        El::entry("text", &draft)
            .class("plugin-search")
            .prop("placeholder", "Type text or a URL… (Enter)")
            .hexpand(true),
        El::button("gen", "")
            .prop("icon", "scanner-symbolic")
            .class("plugin-action plugin-action-primary"),
    ])
    .spacing(8);

    let sources = El::hbox(vec![
        El::button("clip", "Clipboard")
            .prop("icon", "edit-paste-symbolic")
            .class("plugin-chip"),
        El::button("wifi", "Wi-Fi")
            .prop("icon", "network-wireless-symbolic")
            .class("plugin-chip"),
    ])
    .spacing(8)
    .halign("center");

    let mut children = vec![header, input, sources, El::separator()];

    if !error.is_empty() {
        children.push(El::label(error).class("dim-label").halign("center").padding(16));
    } else if qr.is_empty() {
        children.push(
            El::label("Enter text, paste the clipboard, or share your Wi-Fi to make a QR.")
                .class("dim-label")
                .halign("center")
                .padding(24),
        );
    } else {
        children.push(
            El::image(qr)
                .prop("fit", "contain")
                .prop("width", "260")
                .prop("height", "260")
                .halign("center"),
        );
        children.push(
            El::label(truncate(&text, 120)).class("dim-label").halign("center"),
        );
        children.push(
            El::hbox(vec![
                El::button("copytext", "Copy text")
                    .prop("icon", "edit-copy-symbolic")
                    .class("plugin-action plugin-expand"),
                El::button("copyimg", "Copy image")
                    .prop("icon", "image-x-generic-symbolic")
                    .class("plugin-action plugin-expand"),
            ])
            .spacing(8),
        );
    }

    El::vbox(children).spacing(12).class("plugin-panel-large")
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Qr;

impl Component for Qr {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Submit if ev.id == "text" => {
                DRAFT.with(|d| *d.borrow_mut() = ev.value.clone());
                generate(&ev.value);
            }
            EventKind::Input if ev.id == "text" => {
                DRAFT.with(|d| *d.borrow_mut() = ev.value.clone());
            }
            EventKind::Click => match ev.id.as_str() {
                "gen" => {
                    let d = DRAFT.with(|d| d.borrow().clone());
                    generate(&d);
                }
                "clip" => from_clipboard(),
                "wifi" => from_wifi(),
                "copytext" => {
                    let t = TEXT.with(|t| t.borrow().clone());
                    if !t.is_empty() {
                        host::copy(&t);
                        host::notify("QR Generator", "Text copied");
                    }
                }
                "copyimg" => copy_image(),
                "clear" => clear(),
                _ => {}
            },
            _ => {}
        }
        view_tree()
    }
}

export_component!(Qr);
