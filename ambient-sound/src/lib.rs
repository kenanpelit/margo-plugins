//! ambient-sound — an ambient soundscape mixer for the margo shell, written
//! with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A port of Dank Material Shell's `dms-ambient-sound`, reimagined for margo's
//! design language: 14 looping sounds you can mix, a master volume + mute, a
//! row of save/load presets, and a detached sleep timer with a configurable
//! "when done" action.
//!
//! Playback is delegated to a shipped helper script (`ambient.sh`) invoked
//! through the host's `run` capability — it backgrounds one detached `mpv`
//! per sound with a JSON IPC socket so the master volume can be pushed live.
//! State (playing set, volume, presets, timer) is persisted to the plugin's
//! data dir and reconciled with the live `mpv` processes on first open.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

/// One built-in sound: file stem, display name, symbolic icon.
struct Sound {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
}

const SOUNDS: &[Sound] = &[
    Sound { id: "rain", name: "Rain", icon: "weather-rain-symbolic" },
    Sound { id: "storm", name: "Storm", icon: "weather-thunderstorm-symbolic" },
    Sound { id: "wind", name: "Wind", icon: "weather-windy-symbolic" },
    Sound { id: "waves", name: "Waves", icon: "weather-fog-symbolic" },
    Sound { id: "stream", name: "Stream", icon: "weather-drizzle-symbolic" },
    Sound { id: "birds", name: "Birds", icon: "emoji-nature-symbolic" },
    Sound { id: "summer-night", name: "Summer Night", icon: "weather-clear-night-symbolic" },
    Sound { id: "fireplace", name: "Fireplace", icon: "weather-clear-symbolic" },
    Sound { id: "coffee-shop", name: "Coffee Shop", icon: "coffee-symbolic" },
    Sound { id: "city", name: "City", icon: "user-home-symbolic" },
    Sound { id: "train", name: "Train", icon: "emoji-travel-symbolic" },
    Sound { id: "boat", name: "Boat", icon: "weather-overcast-symbolic" },
    Sound { id: "white-noise", name: "White Noise", icon: "audio-volume-high-symbolic" },
    Sound { id: "pink-noise", name: "Pink Noise", icon: "audio-volume-medium-symbolic" },
];

/// Sleep-timer presets (minutes). 0 = off.
const TIMER_PRESETS: &[(u32, &str)] = &[
    (15, "15m"),
    (30, "30m"),
    (45, "45m"),
    (60, "1h"),
    (90, "1.5h"),
    (120, "2h"),
];

const STATE_FILE: &str = "state.json";

#[derive(Clone)]
struct Preset {
    name: String,
    sounds: Vec<String>,
}

thread_local! {
    static MASTER: RefCell<u32> = const { RefCell::new(75) };
    static MUTED: RefCell<bool> = const { RefCell::new(false) };
    static PLAYING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static PRESETS: RefCell<Vec<Preset>> = const { RefCell::new(Vec::new()) };
    /// Selected sleep-timer duration in minutes (0 = off). Display state — the
    /// actual countdown runs detached in the helper script.
    static TIMER_MIN: RefCell<u32> = const { RefCell::new(0) };
    /// When-done action: "stopall" | "lock" | "suspend".
    static WHEN_DONE: RefCell<String> = const { RefCell::new(String::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Helpers: host plumbing ──────────────────────────────────────────────────

fn plugin_dir() -> String {
    host::get_setting("plugin_dir")
}

fn script() -> String {
    format!("{}/ambient.sh", plugin_dir())
}

/// Invoke the helper script with a subcommand + args (blocking, but the script
/// detaches mpv so it returns immediately).
fn helper(cmd: &str, extra: &[&str]) -> host::ProcessOutput {
    let mut args = vec![script(), plugin_dir(), cmd.to_string()];
    args.extend(extra.iter().map(|s| s.to_string()));
    host::run("sh", &args)
}

/// Volume mpv should use right now: 0 when muted, else the master level.
fn effective_volume() -> u32 {
    if MUTED.with(|m| *m.borrow()) {
        0
    } else {
        MASTER.with(|m| *m.borrow())
    }
}

fn when_done() -> String {
    let v = WHEN_DONE.with(|w| w.borrow().clone());
    if v.is_empty() {
        "stopall".to_string()
    } else {
        v
    }
}

// ── Persistence ──────────────────────────────────────────────────────────────

fn load_state() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);

    if let Ok(bytes) = host::read_file(STATE_FILE) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(m) = v["master"].as_u64() {
                MASTER.with(|x| *x.borrow_mut() = (m as u32).min(100));
            }
            if let Some(action) = v["when_done"].as_str() {
                WHEN_DONE.with(|x| *x.borrow_mut() = action.to_string());
            }
            if let Some(arr) = v["presets"].as_array() {
                let presets: Vec<Preset> = arr
                    .iter()
                    .filter_map(|p| {
                        let name = p["name"].as_str()?.to_string();
                        let sounds = p["sounds"]
                            .as_array()?
                            .iter()
                            .filter_map(|s| s.as_str().map(str::to_string))
                            .collect();
                        Some(Preset { name, sounds })
                    })
                    .collect();
                PRESETS.with(|x| *x.borrow_mut() = presets);
            }
        }
    } else {
        // First run: seed a sensible default preset.
        PRESETS.with(|x| {
            *x.borrow_mut() = vec![Preset {
                name: "Relaxing Rain".to_string(),
                sounds: vec!["rain".into(), "birds".into(), "wind".into()],
            }];
        });
    }

    // Reconcile the playing set with the live mpv processes (sounds keep
    // playing after the panel closes; this catches a shell restart too).
    let all_ids: Vec<&str> = SOUNDS.iter().map(|s| s.id).collect();
    let out = helper("status", &all_ids);
    let playing: Vec<String> = out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    PLAYING.with(|x| *x.borrow_mut() = playing);
}

fn save_state() {
    let master = MASTER.with(|m| *m.borrow());
    let presets: Vec<serde_json::Value> = PRESETS.with(|p| {
        p.borrow()
            .iter()
            .map(|pr| serde_json::json!({ "name": pr.name, "sounds": pr.sounds }))
            .collect()
    });
    let value = serde_json::json!({
        "master": master,
        "when_done": when_done(),
        "presets": presets,
    });
    let _ = host::write_file(STATE_FILE, &value.to_string().into_bytes());
}

// ── Audio actions ─────────────────────────────────────────────────────────────

fn is_playing(id: &str) -> bool {
    PLAYING.with(|p| p.borrow().iter().any(|s| s == id))
}

fn toggle_sound(id: &str) {
    if is_playing(id) {
        helper("stop", &[id]);
        PLAYING.with(|p| p.borrow_mut().retain(|s| s != id));
        if PLAYING.with(|p| p.borrow().is_empty()) {
            MUTED.with(|m| *m.borrow_mut() = false);
        }
    } else {
        let vol = effective_volume().to_string();
        helper("play", &[id, &vol]);
        PLAYING.with(|p| p.borrow_mut().push(id.to_string()));
    }
}

fn stop_all() {
    helper("stop-all", &[]);
    PLAYING.with(|p| p.borrow_mut().clear());
    MUTED.with(|m| *m.borrow_mut() = false);
    TIMER_MIN.with(|t| *t.borrow_mut() = 0);
    helper("timer-cancel", &[]);
}

fn push_volume() {
    let vol = effective_volume().to_string();
    helper("vol-all", &[&vol]);
}

fn adjust_master(delta: i32) {
    MASTER.with(|m| {
        let cur = *m.borrow() as i32;
        *m.borrow_mut() = (cur + delta).clamp(0, 100) as u32;
    });
    if !MUTED.with(|m| *m.borrow()) {
        push_volume();
    }
    save_state();
}

fn toggle_mute(on: bool) {
    MUTED.with(|m| *m.borrow_mut() = on);
    push_volume();
}

fn save_preset() {
    let current = PLAYING.with(|p| p.borrow().clone());
    if current.is_empty() {
        host::notify("Ambient Sound", "Play some sounds first to save a preset.");
        return;
    }
    PRESETS.with(|p| {
        let n = p.borrow().len() + 1;
        p.borrow_mut().push(Preset {
            name: format!("Preset {n}"),
            sounds: current,
        });
    });
    save_state();
    host::notify("Ambient Sound", "Saved the current mix as a preset.");
}

fn load_preset(idx: usize) {
    let preset = PRESETS.with(|p| p.borrow().get(idx).cloned());
    let Some(preset) = preset else { return };
    helper("stop-all", &[]);
    MUTED.with(|m| *m.borrow_mut() = false);
    let vol = effective_volume().to_string();
    for s in &preset.sounds {
        helper("play", &[s, &vol]);
    }
    PLAYING.with(|p| *p.borrow_mut() = preset.sounds.clone());
}

fn delete_preset(idx: usize) {
    PRESETS.with(|p| {
        if idx < p.borrow().len() {
            p.borrow_mut().remove(idx);
        }
    });
    save_state();
}

fn set_timer(minutes: u32) {
    TIMER_MIN.with(|t| *t.borrow_mut() = minutes);
    if minutes == 0 {
        helper("timer-cancel", &[]);
    } else {
        let m = minutes.to_string();
        let action = when_done();
        helper("timer", &[&m, &action]);
        host::notify(
            "Ambient Sound",
            &format!("Sleep timer set for {minutes} min."),
        );
    }
}

fn set_when_done(action: &str) {
    WHEN_DONE.with(|w| *w.borrow_mut() = action.to_string());
    save_state();
    // If a timer is already running, restart it with the new action.
    let minutes = TIMER_MIN.with(|t| *t.borrow());
    if minutes > 0 {
        let m = minutes.to_string();
        helper("timer", &[&m, action]);
    }
}

// ── UI ─────────────────────────────────────────────────────────────────────

fn header() -> El {
    let count = PLAYING.with(|p| p.borrow().len());
    let subtitle = match count {
        0 => "Idle — pick a soundscape".to_string(),
        1 => "1 sound playing".to_string(),
        n => format!("{n} sounds playing"),
    };
    let mut right: Vec<El> = Vec::new();
    if count > 0 {
        right.push(
            El::button("act:stopall", "Stop all").class("plugin-action plugin-action-danger"),
        );
    }
    El::hbox(vec![
        El::image("audio-headphones-symbolic"),
        El::vbox(vec![
            El::label("Ambient Sound").class("label-large-bold").halign("start"),
            El::label(subtitle).class("dim-label").halign("start"),
        ])
        .spacing(4)
        .hexpand(true),
        El::hbox(right).spacing(8),
    ])
    .class("plugin-panel-header")
    .spacing(12)
}

fn master_card() -> El {
    let master = MASTER.with(|m| *m.borrow());
    let muted = MUTED.with(|m| *m.borrow());
    let frac = if muted { 0.0 } else { master as f64 / 100.0 };
    let pct = if muted {
        "Muted".to_string()
    } else {
        format!("{master}%")
    };
    El::vbox(vec![
        El::hbox(vec![
            El::label("Master volume").class("dim-label").halign("start").hexpand(true),
            El::label(pct).class("label-large-bold").halign("end"),
        ]),
        El::hbox(vec![
            El::button("vol:down", "")
                .prop("icon", "audio-volume-low-symbolic")
                .class("plugin-action"),
            El::progress(frac).valign("center").hexpand(true),
            El::button("vol:up", "")
                .prop("icon", "audio-volume-high-symbolic")
                .class("plugin-action"),
        ])
        .spacing(12),
        El::hbox(vec![
            El::label("Mute").halign("start").hexpand(true),
            El::switch("mute", muted),
        ]),
    ])
    .spacing(12)
    .padding(16)
    .class("plugin-card")
}

fn sound_tile(s: &Sound) -> El {
    let on = is_playing(s.id);
    let class = if on {
        "plugin-tile plugin-tile-on"
    } else {
        "plugin-tile"
    };
    El::button(format!("s:{}", s.id), s.name)
        .prop("icon", s.icon)
        .class(class)
        // Fill the column so the grid spans the panel width (tiles size to the
        // menu, not their text) instead of hugging the left edge.
        .hexpand(true)
}

fn sounds_section() -> El {
    let tiles: Vec<El> = SOUNDS.iter().map(sound_tile).collect();
    El::vbox(vec![
        El::label("Sounds").class("plugin-section-title").halign("start"),
        El::grid(3, tiles).spacing(8),
    ])
    .spacing(8)
}

fn presets_section() -> El {
    let presets = PRESETS.with(|p| p.borrow().clone());
    let mut rows: Vec<El> = Vec::new();
    for (i, p) in presets.iter().enumerate() {
        let label = format!("{}  ·  {}", p.name, p.sounds.len());
        rows.push(
            El::hbox(vec![
                El::button(format!("preset:load:{i}"), label)
                    .prop("icon", "media-playback-start-symbolic")
                    .class("plugin-action")
                    .hexpand(true),
                El::button(format!("preset:del:{i}"), "")
                    .prop("icon", "user-trash-symbolic")
                    .class("plugin-action plugin-action-danger"),
            ])
            .spacing(8),
        );
    }
    if rows.is_empty() {
        rows.push(El::label("No presets yet — mix some sounds and save.").class("dim-label"));
    }
    let mut children = vec![El::hbox(vec![
        El::label("Presets").class("plugin-section-title").halign("start").hexpand(true),
        El::button("preset:save", "Save mix")
            .prop("icon", "document-save-symbolic")
            .class("plugin-action"),
    ])];
    children.extend(rows);
    El::vbox(children).spacing(8)
}

fn timer_section() -> El {
    let active = TIMER_MIN.with(|t| *t.borrow());
    let mut chips: Vec<El> = Vec::new();
    // "Off" chip first.
    let off_class = if active == 0 {
        "plugin-chip plugin-chip-on"
    } else {
        "plugin-chip"
    };
    chips.push(El::button("timer:0", "Off").class(off_class));
    for (min, label) in TIMER_PRESETS {
        let class = if active == *min {
            "plugin-chip plugin-chip-on"
        } else {
            "plugin-chip"
        };
        chips.push(El::button(format!("timer:{min}"), *label).class(class));
    }

    let done = when_done();
    let done_chip = |value: &str, label: &str| {
        let class = if done == value {
            "plugin-chip plugin-chip-on"
        } else {
            "plugin-chip"
        };
        El::button(format!("done:{value}"), label).class(class)
    };

    El::vbox(vec![
        El::label("Sleep timer").class("plugin-section-title").halign("start"),
        // Durations centred as one row (consistent with the rest of the panel)
        // rather than a left-aligned, ragged 4-column grid.
        El::hbox(chips).spacing(8).halign("center"),
        El::label("When done").class("plugin-section-title").halign("start"),
        El::hbox(vec![
            done_chip("stopall", "Stop"),
            done_chip("lock", "Lock"),
            done_chip("suspend", "Suspend"),
        ])
        .spacing(8)
        .halign("center"),
    ])
    .spacing(8)
}

fn view_tree() -> El {
    load_state();
    El::scroll(vec![El::vbox(vec![
        header(),
        master_card(),
        sounds_section(),
        El::separator(),
        presets_section(),
        El::separator(),
        timer_section(),
    ])
    .spacing(16)])
    .vexpand(true)
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Ambient;

impl Component for Ambient {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(sound) = id.strip_prefix("s:") {
                    toggle_sound(sound);
                } else if let Some(rest) = id.strip_prefix("preset:load:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        load_preset(i);
                    }
                } else if let Some(rest) = id.strip_prefix("preset:del:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        delete_preset(i);
                    }
                } else if let Some(rest) = id.strip_prefix("timer:") {
                    if let Ok(m) = rest.parse::<u32>() {
                        set_timer(m);
                    }
                } else if let Some(action) = id.strip_prefix("done:") {
                    set_when_done(action);
                } else {
                    match id {
                        "act:stopall" => stop_all(),
                        "vol:down" => adjust_master(-5),
                        "vol:up" => adjust_master(5),
                        "preset:save" => save_preset(),
                        "mute" => toggle_mute(ev.value == "true"),
                        _ => {}
                    }
                }
            }
            EventKind::Keybind => {
                // The panel is already opening when "open" fires — nothing to do.
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Ambient);
