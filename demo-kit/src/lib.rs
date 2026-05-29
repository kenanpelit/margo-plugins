//! Demo Kit — reference plugin exercising every node kind and host capability
//! the WASM tier ships. Read this file as a guided tour of what `mplugin-sdk`
//! 0.1 can do; the panel itself shows the result.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    /// Slider value (also drives the progress bar).
    static VOLUME: RefCell<f64> = const { RefCell::new(50.0) };
    /// Switch state.
    static FANCY: RefCell<bool> = const { RefCell::new(true) };
    /// Last-saved counter (loaded from disk on first view).
    static SAVED: RefCell<u32> = const { RefCell::new(0) };
    /// Last clipboard text fetched by "Paste from clipboard".
    static PASTED: RefCell<String> = const { RefCell::new(String::new()) };
    /// True once we've tried to load the counter from disk.
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

struct DemoKit;

fn ensure_loaded() {
    LOADED.with(|l| {
        if *l.borrow() {
            return;
        }
        if let Ok(bytes) = host::read_file("counter.txt") {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                if let Ok(n) = text.trim().parse::<u32>() {
                    SAVED.with(|s| *s.borrow_mut() = n);
                }
            }
        }
        *l.borrow_mut() = true;
    });
}

fn save_counter(n: u32) {
    let _ = host::write_file("counter.txt", n.to_string().as_bytes());
}

fn section(title: &str, body: Vec<El>) -> El {
    let mut children = vec![El::label(title).class("label-medium-bold")];
    children.extend(body);
    El::vbox(children).spacing(8).padding(8)
}

fn view_tree() -> El {
    ensure_loaded();
    let volume = VOLUME.with(|v| *v.borrow());
    let fancy = FANCY.with(|f| *f.borrow());
    let saved = SAVED.with(|s| *s.borrow());
    let pasted = PASTED.with(|p| p.borrow().clone());

    El::vbox(vec![
        // ── Hero ────────────────────────────────────────────────────────
        El::markdown(format!(
            "**Demo Kit** — every node + capability\n\
             Slider: `{volume:.0}` · Fancy: `{}` · Saved counter: `{saved}`",
            if fancy { "on" } else { "off" }
        ))
        .class("plugin-hero plugin-hero-on"),
        // ── Node-kind showcase ──────────────────────────────────────────
        section(
            "Images",
            vec![El::hbox(vec![
                El::image("audio-volume-high-symbolic"),
                El::image("network-wireless-signal-good-symbolic"),
                El::image("weather-clear-symbolic"),
                El::image("battery-good-symbolic"),
            ])
            .spacing(12)
            .halign("center")],
        ),
        El::separator(),
        section(
            "Switch + slider + progress",
            vec![
                El::hbox(vec![
                    El::label("Fancy mode").hexpand(true),
                    El::switch("fancy", fancy),
                ])
                .spacing(8),
                El::slider("volume", 0.0, 100.0, volume)
                    .prop("step", "1")
                    .hexpand(true),
                El::progress(volume / 100.0).hexpand(true),
            ],
        ),
        El::separator(),
        // ── Capability showcase ─────────────────────────────────────────
        section(
            "Scoped filesystem",
            vec![
                El::label(format!(
                    "counter.txt under ~/.local/share/mshell/plugins/demo-kit/ → {saved}"
                ))
                .class("dim-label"),
                El::hbox(vec![
                    El::button("save", format!("Save {} → disk", (volume as u32))).hexpand(true),
                    El::button("load", "Re-load from disk").hexpand(true),
                ])
                .spacing(8),
            ],
        ),
        El::separator(),
        section(
            "Clipboard round-trip",
            vec![
                El::hbox(vec![
                    El::button("paste", "Paste from clipboard").hexpand(true),
                    El::button("copy", "Copy panel state").hexpand(true),
                ])
                .spacing(8),
                if pasted.is_empty() {
                    El::label("(no clipboard read yet)").class("dim-label")
                } else {
                    El::markdown(format!("**Clipboard:** {pasted}"))
                },
            ],
        ),
    ])
    .spacing(12)
    .padding(12)
    .class("plugin-panel-body")
}

impl Component for DemoKit {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => match ev.id.as_str() {
                "fancy" => {
                    FANCY.with(|f| *f.borrow_mut() = ev.value == "true");
                }
                "save" => {
                    let v = VOLUME.with(|v| *v.borrow()) as u32;
                    SAVED.with(|s| *s.borrow_mut() = v);
                    save_counter(v);
                    host::notify("Demo Kit", &format!("Saved counter = {v}"));
                }
                "load" => {
                    // Re-read regardless of the cache.
                    LOADED.with(|l| *l.borrow_mut() = false);
                    ensure_loaded();
                }
                "paste" => {
                    let text = host::clipboard_read();
                    PASTED.with(|p| *p.borrow_mut() = text);
                }
                "copy" => {
                    let v = VOLUME.with(|v| *v.borrow()) as u32;
                    let f = FANCY.with(|f| *f.borrow());
                    host::copy(&format!("volume={v} fancy={f}"));
                    host::notify("Demo Kit", "Panel state copied to clipboard.");
                }
                _ => {}
            },
            EventKind::Input if ev.id == "volume" => {
                if let Ok(v) = ev.value.parse::<f64>() {
                    VOLUME.with(|x| *x.borrow_mut() = v);
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(DemoKit);
