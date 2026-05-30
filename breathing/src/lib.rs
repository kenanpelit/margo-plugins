//! breathing — a guided breathing-exercise panel for the margo shell, written
//! with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A port of Dank Material Shell's `dms-breathing`, restyled to margo's design
//! language with a live **breathing orb** (the `breath-orb` extended node) that
//! grows on the inhale and shrinks on the exhale, phase coaching, a per-phase
//! countdown, and a session timer.
//!
//! ## Driving the animation without a timer capability
//!
//! The WASM tier has no timer, and the host re-renders the whole tree on every
//! event. So the panel starts a detached "ticker" subprocess via `process-start`
//! that prints a newline every 100 ms; each newline arrives as a `stream-chunk`
//! and advances a tick counter (10 ticks = 1 s). Phase + countdown + orb size
//! are derived from that counter, so the breathing rhythm is paced by ticks
//! (stable) rather than wall-clock. The ticker carries a unique marker in its
//! argv so it can be killed with `pkill` on stop/finish — no zombie left behind.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

/// A breathing technique. Phase durations are in **ticks** (10 ticks = 1 s).
struct Tech {
    name: &'static str,
    desc: &'static str,
    icon: &'static str,
    inhale: u32,
    hold: u32,
    exhale: u32,
    rest: u32, // hold-after-exhale
}

const TECHNIQUES: &[Tech] = &[
    Tech { name: "Deep Breathing", desc: "Balanced calm", icon: "weather-windy-symbolic",
           inhale: 40, hold: 40, exhale: 40, rest: 0 },
    Tech { name: "4-7-8 Breathing", desc: "Calming for sleep", icon: "weather-clear-night-symbolic",
           inhale: 40, hold: 70, exhale: 80, rest: 0 },
    Tech { name: "Box Breathing", desc: "Steady focus", icon: "view-grid-symbolic",
           inhale: 40, hold: 40, exhale: 40, rest: 40 },
    Tech { name: "Equal Breathing", desc: "Even in & out", icon: "media-playlist-repeat-symbolic",
           inhale: 40, hold: 0, exhale: 40, rest: 0 },
    Tech { name: "Resonance", desc: "Heart-rate coherence", icon: "emoji-nature-symbolic",
           inhale: 55, hold: 0, exhale: 55, rest: 0 },
    Tech { name: "Alternate Nostril", desc: "Nervous-system reset", icon: "weather-fog-symbolic",
           inhale: 40, hold: 0, exhale: 40, rest: 20 },
];

const DURATIONS: &[u32] = &[1, 3, 5, 10, 15, 30]; // minutes
const TICKER_MARK: &str = "margobreathtick";

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Inhale,
    Hold,
    Exhale,
    Rest,
}

impl Phase {
    fn word(self) -> &'static str {
        match self {
            Phase::Inhale => "Breathe In",
            Phase::Hold => "Hold",
            Phase::Exhale => "Breathe Out",
            Phase::Rest => "Rest",
        }
    }
    fn class(self) -> &'static str {
        match self {
            Phase::Inhale => "breath-orb-inhale",
            Phase::Hold => "breath-orb-hold",
            Phase::Exhale => "breath-orb-exhale",
            Phase::Rest => "breath-orb-rest",
        }
    }
    /// Orb openness across this phase: `t` is ticks into the phase, `len` its
    /// length. Inhale fills 0→1, exhale empties 1→0, hold stays full, rest stays
    /// empty.
    fn fraction(self, t: u32, len: u32) -> f64 {
        let p = if len == 0 { 0.0 } else { t as f64 / len as f64 };
        match self {
            Phase::Inhale => p,
            Phase::Hold => 1.0,
            Phase::Exhale => 1.0 - p,
            Phase::Rest => 0.0,
        }
    }
}

thread_local! {
    static TECH: RefCell<usize> = const { RefCell::new(0) };
    static MINUTES: RefCell<u32> = const { RefCell::new(0) };
    static RUNNING: RefCell<bool> = const { RefCell::new(false) };
    static PAUSED: RefCell<bool> = const { RefCell::new(false) };
    /// Elapsed ticks while running (10 = 1 s).
    static TICKS: RefCell<u32> = const { RefCell::new(0) };
    /// req id of the live ticker stream — chunks from any other id are ignored.
    static TICKER: RefCell<String> = const { RefCell::new(String::new()) };
    /// The phase shown on the previous render — used to fire a chime exactly on
    /// each inhale transition.
    static LAST_PHASE: RefCell<i32> = const { RefCell::new(-1) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Settings / init ──────────────────────────────────────────────────────────

fn ensure_loaded() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    // Seed the duration from the user's default.
    let m: u32 = host::get_setting("default_minutes").trim().parse().unwrap_or(5);
    let m = if DURATIONS.contains(&m) { m } else { 5 };
    MINUTES.with(|x| *x.borrow_mut() = m);
}

fn cues_enabled() -> bool {
    matches!(
        host::get_setting("cues").to_lowercase().as_str(),
        "yes" | "true" | "1" | "on"
    )
}

fn tech() -> &'static Tech {
    let i = TECH.with(|t| *t.borrow()).min(TECHNIQUES.len() - 1);
    &TECHNIQUES[i]
}

/// The non-zero phases of the current technique, in order, with their lengths.
fn phases() -> Vec<(Phase, u32)> {
    let t = tech();
    [
        (Phase::Inhale, t.inhale),
        (Phase::Hold, t.hold),
        (Phase::Exhale, t.exhale),
        (Phase::Rest, t.rest),
    ]
    .into_iter()
    .filter(|(_, len)| *len > 0)
    .collect()
}

fn cycle_len() -> u32 {
    phases().iter().map(|(_, l)| *l).sum::<u32>().max(1)
}

fn total_ticks() -> u32 {
    MINUTES.with(|m| *m.borrow()) * 60 * 10
}

/// Current (phase, ticks-into-phase, phase-length) for the elapsed tick count.
fn current_phase() -> (Phase, u32, u32) {
    let ph = phases();
    let mut pos = TICKS.with(|t| *t.borrow()) % cycle_len();
    for (phase, len) in &ph {
        if pos < *len {
            return (*phase, pos, *len);
        }
        pos -= *len;
    }
    // Fallback (shouldn't happen): first phase.
    ph.first().map(|(p, l)| (*p, 0, *l)).unwrap_or((Phase::Inhale, 0, 1))
}

// ── Ticker / audio plumbing ──────────────────────────────────────────────────

fn start_ticker() {
    stop_ticker(); // belt-and-suspenders: never run two
    // 500 ms heartbeat: each line advances 5 ticks (1 tick = 0.1 s). A slower
    // beat means the panel re-renders only twice a second instead of ten times,
    // so the Pause / End buttons aren't torn down mid-click — the host rebuilds
    // the whole tree on every event, and a 100 ms beat made them near-unclickable.
    let script = format!(": {TICKER_MARK}; while :; do printf '.\\n'; sleep 0.5; done");
    let id = host::process_start("sh", &["-c".to_string(), script]);
    TICKER.with(|t| *t.borrow_mut() = id);
}

fn stop_ticker() {
    let _ = host::run("pkill", &["-f".to_string(), TICKER_MARK.to_string()]);
    TICKER.with(|t| t.borrow_mut().clear());
}

fn play_chime() {
    if !cues_enabled() {
        return;
    }
    let dir = host::get_setting("plugin_dir");
    if dir.is_empty() {
        return;
    }
    // Background it so the host's blocking `run` returns immediately.
    let cmd = format!("paplay '{dir}/sounds/chime.ogg' >/dev/null 2>&1 &");
    let _ = host::run("sh", &["-c".to_string(), cmd]);
}

// ── Actions ──────────────────────────────────────────────────────────────────

fn start_exercise() {
    TICKS.with(|t| *t.borrow_mut() = 0);
    PAUSED.with(|p| *p.borrow_mut() = false);
    LAST_PHASE.with(|p| *p.borrow_mut() = -1);
    RUNNING.with(|r| *r.borrow_mut() = true);
    start_ticker();
}

fn stop_exercise() {
    RUNNING.with(|r| *r.borrow_mut() = false);
    PAUSED.with(|p| *p.borrow_mut() = false);
    stop_ticker();
}

fn finish_exercise() {
    RUNNING.with(|r| *r.borrow_mut() = false);
    stop_ticker();
    play_chime();
    host::notify("Breathing", "Session complete — well done.");
}

/// One tick advanced the timer: bump the phase chime + completion check.
fn on_advance() {
    if TICKS.with(|t| *t.borrow()) >= total_ticks() {
        finish_exercise();
        return;
    }
    // Chime on each inhale transition.
    let (phase, into, _) = current_phase();
    let idx = phase as i32;
    let last = LAST_PHASE.with(|p| *p.borrow());
    if into == 0 && idx != last && phase == Phase::Inhale {
        play_chime();
    }
    LAST_PHASE.with(|p| *p.borrow_mut() = idx);
}

// ── UI: selector ─────────────────────────────────────────────────────────────

fn technique_tile(i: usize, t: &Tech) -> El {
    let selected = TECH.with(|x| *x.borrow()) == i;
    let class = if selected {
        "plugin-tile plugin-tile-on"
    } else {
        "plugin-tile"
    };
    El::button(format!("tech:{i}"), t.name)
        .prop("icon", t.icon)
        .class(class)
        // Fill the column so the grid spans the panel width (and the tiles
        // size to the menu, not their text), instead of hugging the left.
        .hexpand(true)
}

fn selector_view() -> El {
    let t = tech();
    let cycle_sec = (t.inhale + t.hold + t.exhale + t.rest) as f64 / 10.0;
    let pattern = {
        let mut parts = vec![format!("{}s in", t.inhale / 10)];
        if t.hold > 0 {
            parts.push(format!("{}s hold", t.hold / 10));
        }
        parts.push(format!("{}s out", t.exhale / 10));
        if t.rest > 0 {
            parts.push(format!("{}s rest", t.rest / 10));
        }
        parts.join(" · ")
    };

    let tiles: Vec<El> = TECHNIQUES.iter().enumerate().map(|(i, t)| technique_tile(i, t)).collect();

    let cur_min = MINUTES.with(|m| *m.borrow());
    let dur_chips: Vec<El> = DURATIONS
        .iter()
        .map(|m| {
            let class = if *m == cur_min {
                "plugin-chip plugin-chip-on"
            } else {
                "plugin-chip"
            };
            let label = if *m >= 60 { format!("{}h", m / 60) } else { format!("{m}m") };
            El::button(format!("dur:{m}"), label).class(class)
        })
        .collect();

    El::scroll(vec![El::vbox(vec![
        El::hbox(vec![
            El::image("weather-windy-symbolic"),
            El::vbox(vec![
                El::label("Breathing").class("label-large-bold").halign("start"),
                El::label("Pick a technique, then breathe").class("dim-label").halign("start"),
            ])
            .spacing(2)
            .hexpand(true),
        ])
        .class("plugin-panel-header")
        .spacing(10),
        El::label("Technique").class("plugin-section-title").halign("start"),
        El::grid(2, tiles).spacing(8),
        // Selected-technique summary card.
        El::vbox(vec![
            El::label(t.name).class("label-medium-bold").halign("start"),
            El::label(format!("{} — {pattern} · {cycle_sec:.0}s cycle", t.desc))
                .class("dim-label")
                .halign("start"),
        ])
        .spacing(4)
        .padding(12)
        .class("plugin-card"),
        El::label("Duration").class("plugin-section-title").halign("start"),
        El::hbox(dur_chips).spacing(6).halign("center"),
        El::button("start", "Start session")
            .prop("icon", "media-playback-start-symbolic")
            .class("plugin-action plugin-action-primary plugin-expand")
            .hexpand(true),
    ])
    .spacing(14)])
    .vexpand(true)
}

// ── UI: active exercise ───────────────────────────────────────────────────────

fn exercise_view() -> El {
    let (phase, into, len) = current_phase();
    let frac = phase.fraction(into, len);
    let phase_remaining = (len - into).div_ceil(10); // seconds, rounded up
    let total = total_ticks();
    let elapsed = TICKS.with(|t| *t.borrow());
    let overall_remaining = total.saturating_sub(elapsed) / 10;
    let mm = overall_remaining / 60;
    let ss = overall_remaining % 60;
    let paused = PAUSED.with(|p| *p.borrow());

    let phase_word = if paused { "Paused" } else { phase.word() };
    let orb_class = phase.class();

    El::vbox(vec![
        // Header: technique + session time left.
        El::hbox(vec![
            El::vbox(vec![
                El::label(tech().name).class("label-large-bold").halign("start"),
                El::label(format!("{mm}:{ss:02} left")).class("dim-label").halign("start"),
            ])
            .spacing(2)
            .hexpand(true),
            El::label(format!("{}m", MINUTES.with(|m| *m.borrow())))
                .class("plugin-status-badge"),
        ])
        .class("plugin-panel-header")
        .spacing(10),
        // The orb — the centrepiece.
        El::breath_orb(frac, 200).class(orb_class).halign("center"),
        // Phase word + per-phase countdown.
        El::vbox(vec![
            El::label(phase_word).class(format!("breath-phase {orb_class}")).halign("center"),
            El::label(phase_remaining.to_string()).class("breath-count").halign("center"),
        ])
        .spacing(2),
        // Controls.
        El::hbox(vec![
            El::button("pause", if paused { "Resume" } else { "Pause" })
                .prop("icon", if paused { "media-playback-start-symbolic" } else { "media-playback-pause-symbolic" })
                .class("plugin-action plugin-expand")
                .hexpand(true),
            El::button("stop", "End")
                .prop("icon", "media-playback-stop-symbolic")
                .class("plugin-action plugin-action-danger plugin-expand")
                .hexpand(true),
        ])
        .spacing(8),
    ])
    .spacing(14)
    .padding(8)
}

fn view_tree() -> El {
    ensure_loaded();
    if RUNNING.with(|r| *r.borrow()) {
        exercise_view()
    } else {
        selector_view()
    }
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Breathing;

impl Component for Breathing {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(rest) = id.strip_prefix("tech:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        TECH.with(|t| *t.borrow_mut() = i.min(TECHNIQUES.len() - 1));
                    }
                } else if let Some(rest) = id.strip_prefix("dur:") {
                    if let Ok(m) = rest.parse::<u32>() {
                        MINUTES.with(|x| *x.borrow_mut() = m);
                    }
                } else {
                    match id {
                        "start" => start_exercise(),
                        "stop" => stop_exercise(),
                        "pause" => {
                            let now = !PAUSED.with(|p| *p.borrow());
                            PAUSED.with(|p| *p.borrow_mut() = now);
                            // Killing the ticker while paused means the panel
                            // stops re-rendering entirely, so Resume / End are
                            // 100% reliable; restart it on resume (without
                            // resetting the elapsed ticks).
                            if now {
                                stop_ticker();
                            } else {
                                start_ticker();
                            }
                        }
                        _ => {}
                    }
                }
            }
            EventKind::StreamChunk => {
                // Only the live ticker drives the clock.
                let is_current = TICKER.with(|t| *t.borrow() == ev.id);
                if is_current
                    && RUNNING.with(|r| *r.borrow())
                    && !PAUSED.with(|p| *p.borrow())
                {
                    // Each 500 ms heartbeat line is 5 ticks (1 tick = 0.1 s).
                    let advanced = ev.value.matches('\n').count() as u32 * 5;
                    if advanced > 0 {
                        TICKS.with(|t| *t.borrow_mut() += advanced);
                        on_advance();
                    }
                }
            }
            EventKind::StreamEnd => {
                // The ticker died (we killed it, or it ended). If we're still
                // marked running with the timer expired, treat as finished.
                if TICKER.with(|t| *t.borrow() == ev.id) {
                    TICKER.with(|t| t.borrow_mut().clear());
                }
            }
            EventKind::Keybind => {}
            _ => {}
        }
        view_tree()
    }
}

export_component!(Breathing);
