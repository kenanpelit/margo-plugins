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
    /// Whether the "advanced" revealer is expanded.
    static SHOW_ADVANCED: RefCell<bool> = const { RefCell::new(false) };
    /// Which stack page is on top: "controls" or "about".
    static TAB: RefCell<&'static str> = const { RefCell::new("controls") };
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

fn controls_pane() -> El {
    let volume = VOLUME.with(|v| *v.borrow());
    let fancy = FANCY.with(|f| *f.borrow());
    let saved = SAVED.with(|s| *s.borrow());
    let pasted = PASTED.with(|p| p.borrow().clone());
    let show_adv = SHOW_ADVANCED.with(|s| *s.borrow());

    El::vbox(vec![
        // ── Grid showcase: 2×2 image grid ───────────────────────────────
        section(
            "Images in a 2×2 grid",
            vec![El::grid(
                2,
                vec![
                    El::image("audio-volume-high-symbolic").halign("center"),
                    El::image("network-wireless-signal-good-symbolic").halign("center"),
                    El::image("weather-clear-symbolic").halign("center"),
                    El::image("battery-good-symbolic").halign("center"),
                ],
            )
            .halign("center")],
        ),
        El::separator(),
        // ── Switch + slider + progress ──────────────────────────────────
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
        // ── Revealer showcase ───────────────────────────────────────────
        El::hbox(vec![
            El::label("Advanced").hexpand(true),
            El::button(
                "toggle-adv",
                if show_adv { "Hide" } else { "Show" },
            ),
        ]),
        El::revealer(
            show_adv,
            El::vbox(vec![
                section(
                    "Scoped filesystem",
                    vec![
                        El::label(format!(
                            "counter.txt under ~/.local/share/mshell/plugins/demo-kit/ → {saved}"
                        ))
                        .class("dim-label"),
                        El::hbox(vec![
                            El::button("save", format!("Save {} → disk", (volume as u32)))
                                .hexpand(true),
                            El::button("load", "Re-load from disk").hexpand(true),
                        ])
                        .spacing(8),
                    ],
                ),
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
                section("Live system snapshot", {
                    let sys = host::system_state();
                    let media = host::media_now_playing();
                    let battery = if sys.battery_pct == 255 {
                        "no battery".to_string()
                    } else {
                        format!("{}% · {}", sys.battery_pct, sys.battery_status)
                    };
                    let track = if media.title.trim().is_empty() {
                        "no media".to_string()
                    } else if media.artist.trim().is_empty() {
                        media.title.clone()
                    } else {
                        format!("{} — {}", media.title, media.artist)
                    };
                    vec![
                        El::markdown(format!("**Battery:** {battery}")),
                        El::markdown(format!("**Now playing ({}):** {track}", media.status)),
                        El::button("refresh-sys", "Refresh snapshot").hexpand(true),
                    ]
                }),
            ])
            .spacing(8),
        )
        .prop("transition", "slide-down"),
    ])
    .spacing(12)
    .with_id("controls")
}

fn about_pane() -> El {
    El::vbox(vec![El::markdown(
        "**Demo Kit** is the reference plugin for the WASM tier.\n\n\
         • Every node kind exercised here (`vbox`/`hbox`/`grid`/`stack`/`revealer`/`label`/\n\
         `markdown`/`button`/`entry`/`switch`/`slider`/`progress`/`separator`/`image`).\n\
         • Every host capability called (`log`/`get-setting`/`notify`/`copy`/`clipboard-read`/\n\
         `read-file`/`write-file`/`http`/`http-start`/`process-start`/`run`).\n\n\
         Read this plugin's `src/lib.rs` as a guided tour of `mplugin-sdk`.",
    )])
    .padding(8)
    .with_id("about")
}

fn view_tree() -> El {
    ensure_loaded();
    let volume = VOLUME.with(|v| *v.borrow());
    let fancy = FANCY.with(|f| *f.borrow());
    let saved = SAVED.with(|s| *s.borrow());
    let tab = TAB.with(|t| *t.borrow());

    El::vbox(vec![
        // Hero
        El::markdown(format!(
            "**Demo Kit** — every node + capability\n\
             Slider: `{volume:.0}` · Fancy: `{}` · Saved counter: `{saved}`",
            if fancy { "on" } else { "off" }
        ))
        .class("plugin-hero plugin-hero-on"),
        // Stack tabs
        El::hbox(vec![
            El::button("tab-controls", "Controls")
                .class(if tab == "controls" {
                    "plugin-toggle plugin-toggle-on plugin-expand"
                } else {
                    "plugin-toggle plugin-expand"
                }),
            El::button("tab-about", "About")
                .class(if tab == "about" {
                    "plugin-toggle plugin-toggle-on plugin-expand"
                } else {
                    "plugin-toggle plugin-expand"
                }),
        ])
        .spacing(8),
        El::stack(tab, vec![controls_pane(), about_pane()]),
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
                "tab-controls" => TAB.with(|t| *t.borrow_mut() = "controls"),
                "tab-about" => TAB.with(|t| *t.borrow_mut() = "about"),
                "toggle-adv" => SHOW_ADVANCED.with(|s| {
                    let mut s = s.borrow_mut();
                    *s = !*s;
                }),
                "refresh-sys" => { /* view() re-reads everything */ }
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
