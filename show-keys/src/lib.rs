//! Show Keys — live keyboard input display via `evtest`, ported from the
//! noctalia plugin. Drives the host's `process-start` capability to stream
//! evtest stdout into the panel; parses `(KEY_FOO)` + `value N` lines and
//! maintains a rolling window of recent keys with modifier prefixes.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    static KEYS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CAPTURING: RefCell<bool> = const { RefCell::new(false) };
    static LINE_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    static REQ_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static MOD_SHIFT: RefCell<bool> = const { RefCell::new(false) };
    static MOD_CTRL: RefCell<bool> = const { RefCell::new(false) };
    static MOD_ALT: RefCell<bool> = const { RefCell::new(false) };
    static MOD_META: RefCell<bool> = const { RefCell::new(false) };
}

fn max_keys() -> usize {
    let raw = host::get_setting("max_keys");
    raw.trim().parse::<usize>().unwrap_or(12).max(1).min(64)
}

fn device() -> String {
    let d = host::get_setting("device");
    if d.trim().is_empty() {
        "/dev/input/event3".to_string()
    } else {
        d
    }
}

fn start_capture() {
    if CAPTURING.with(|c| *c.borrow()) {
        return;
    }
    let dev = device();
    let id = host::process_start("evtest", &[dev]);
    REQ_ID.with(|r| *r.borrow_mut() = id);
    CAPTURING.with(|c| *c.borrow_mut() = true);
    LINE_BUF.with(|b| b.borrow_mut().clear());
}

fn stop_capture() {
    // process-start has no kill capability today; flipping the flag makes
    // ingest_chunk discard incoming data so the UI stops updating. The
    // child process is reaped when the panel is reloaded (or mshell exits).
    CAPTURING.with(|c| *c.borrow_mut() = false);
}

/// Pretty-print a KEY_FOO suffix for the pill — lower-case + a handful of
/// arrows/punctuation/named keys rendered as glyphs.
fn pretty_key(k: &str) -> String {
    match k {
        "UP" => "↑".to_string(),
        "DOWN" => "↓".to_string(),
        "LEFT" => "←".to_string(),
        "RIGHT" => "→".to_string(),
        "ENTER" => "↵".to_string(),
        "TAB" => "⇥".to_string(),
        "BACKSPACE" => "⌫".to_string(),
        "ESC" => "ESC".to_string(),
        "SPACE" => "␣".to_string(),
        "CAPSLOCK" => "⇪".to_string(),
        "MINUS" => "-".to_string(),
        "EQUAL" => "=".to_string(),
        "LEFTBRACE" => "[".to_string(),
        "RIGHTBRACE" => "]".to_string(),
        "BACKSLASH" => "\\".to_string(),
        "SEMICOLON" => ";".to_string(),
        "APOSTROPHE" => "'".to_string(),
        "GRAVE" => "`".to_string(),
        "COMMA" => ",".to_string(),
        "DOT" => ".".to_string(),
        "SLASH" => "/".to_string(),
        other => other.to_lowercase(),
    }
}

fn parse_evtest_line(line: &str) {
    if !line.contains("type 1 (EV_KEY)") {
        return;
    }
    let Some(open) = line.find("(KEY_") else {
        return;
    };
    let Some(close_off) = line[open + 5..].find(')') else {
        return;
    };
    let key = &line[open + 5..open + 5 + close_off];
    let Some(val_pos) = line.find("value ") else {
        return;
    };
    let val_str: String = line[val_pos + 6..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let val: u32 = val_str.parse().unwrap_or(99);
    if val == 2 {
        return; // ignore repeats
    }
    let is_press = val == 1;

    match key {
        "LEFTSHIFT" | "RIGHTSHIFT" => {
            MOD_SHIFT.with(|m| *m.borrow_mut() = is_press);
            return;
        }
        "LEFTCTRL" | "RIGHTCTRL" => {
            MOD_CTRL.with(|m| *m.borrow_mut() = is_press);
            return;
        }
        "LEFTALT" | "RIGHTALT" => {
            MOD_ALT.with(|m| *m.borrow_mut() = is_press);
            return;
        }
        "LEFTMETA" | "RIGHTMETA" => {
            MOD_META.with(|m| *m.borrow_mut() = is_press);
            return;
        }
        _ => {}
    }

    if !is_press {
        return;
    }

    let mut prefix = String::new();
    if MOD_META.with(|m| *m.borrow()) {
        prefix.push_str("⌘+");
    }
    if MOD_CTRL.with(|m| *m.borrow()) {
        prefix.push_str("Ctrl+");
    }
    if MOD_ALT.with(|m| *m.borrow()) {
        prefix.push_str("Alt+");
    }
    if MOD_SHIFT.with(|m| *m.borrow()) {
        prefix.push_str("⇧+");
    }
    let display = format!("{prefix}{}", pretty_key(key));
    let cap = max_keys();
    KEYS.with(|k| {
        let mut v = k.borrow_mut();
        v.push(display);
        if v.len() > cap {
            let excess = v.len() - cap;
            v.drain(..excess);
        }
    });
}

fn ingest_chunk(chunk: &str) {
    if !CAPTURING.with(|c| *c.borrow()) {
        return;
    }
    let mut accum = LINE_BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
    accum.push_str(chunk);
    // Drain whole lines; keep the dangling tail for next chunk.
    let mut last = 0usize;
    let mut lines: Vec<String> = Vec::new();
    for (i, ch) in accum.char_indices() {
        if ch == '\n' {
            lines.push(accum[last..i].to_string());
            last = i + 1;
        }
    }
    let tail = accum[last..].to_string();
    LINE_BUF.with(|b| *b.borrow_mut() = tail);
    for line in lines {
        parse_evtest_line(&line);
    }
}

struct ShowKeys;

fn view_tree() -> El {
    let capturing = CAPTURING.with(|c| *c.borrow());
    let keys = KEYS.with(|k| k.borrow().clone());
    let dev = device();

    let (status_label, status_class) = if capturing {
        ("Capturing", "plugin-status-badge plugin-status-ok")
    } else {
        ("Stopped", "plugin-status-badge")
    };

    let header = El::hbox(vec![
        El::image("input-keyboard-symbolic"),
        El::label("Show Keys").hexpand(true).halign("start"),
        El::label(status_label).class(status_class).halign("end"),
    ])
    .spacing(8)
    .class("plugin-panel-header");

    let body: El = if keys.is_empty() {
        let hint = if capturing {
            "Press a key to see it here."
        } else {
            "Tap Start to begin capturing."
        };
        El::label(hint)
            .class("dim-label")
            .halign("center")
            .padding(24)
    } else {
        let pills: Vec<El> = keys
            .iter()
            .map(|k| El::label(k.clone()).class("plugin-key-pill"))
            .collect();
        El::hbox(pills)
            .spacing(6)
            .halign("center")
            .padding(12)
            .class("plugin-card")
    };

    let (toggle_label, toggle_class) = if capturing {
        ("Stop", "plugin-action plugin-action-danger plugin-expand")
    } else {
        ("Start", "plugin-action plugin-action-primary plugin-expand")
    };

    El::vbox(vec![
        header,
        body,
        El::separator(),
        El::hbox(vec![
            El::button("toggle", toggle_label).class(toggle_class),
            El::button("clear", "Clear").class("plugin-action plugin-expand"),
        ])
        .spacing(8),
        El::label(format!("Device · {dev}"))
            .class("dim-label")
            .halign("center"),
        El::label("Requires evtest + your user in the `input` group.")
            .class("dim-label")
            .halign("center"),
    ])
    .spacing(12)
    .class("plugin-panel-large")
}

impl Component for ShowKeys {
    fn view() -> El {
        view_tree()
    }
    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => match ev.id.as_str() {
                "toggle" => {
                    if CAPTURING.with(|c| *c.borrow()) {
                        stop_capture();
                    } else {
                        start_capture();
                    }
                }
                "clear" => KEYS.with(|k| k.borrow_mut().clear()),
                _ => {}
            },
            EventKind::Keybind if ev.id == "toggle" => {
                if CAPTURING.with(|c| *c.borrow()) {
                    stop_capture();
                } else {
                    start_capture();
                }
            }
            EventKind::StreamChunk => {
                ingest_chunk(&ev.value);
            }
            EventKind::StreamEnd => {
                CAPTURING.with(|c| *c.borrow_mut() = false);
                LINE_BUF.with(|b| b.borrow_mut().clear());
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(ShowKeys);
