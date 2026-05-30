//! mplayerplus — a full media-player panel for the margo shell, written with
//! the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A reimagining of Dank Material Shell's MediaControlPlus, focused on actual
//! controls rather than a visualizer: album art, a live progress bar, and
//! play/pause · prev/next · ±N s seek · shuffle · repeat — driving the active
//! MPRIS player through `playerctl`. State comes from the host's
//! `media-now-playing` (now wired with live position + the player name); a
//! 1 Hz heartbeat advances the progress readout.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

const TICKER_MARK: &str = "margomplayertick";

thread_local! {
    static TICKER: RefCell<String> = const { RefCell::new(String::new()) };
    static SHUFFLE: RefCell<bool> = const { RefCell::new(false) };
    /// "none" | "track" | "playlist"
    static LOOP: RefCell<String> = const { RefCell::new(String::new()) };
    static PLAYER: RefCell<String> = const { RefCell::new(String::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Settings ──────────────────────────────────────────────────────────────

fn seek_step() -> u32 {
    host::get_setting("seek_step").trim().parse().unwrap_or(10).max(1)
}

fn show_art() -> bool {
    !matches!(host::get_setting("show_art").to_lowercase().as_str(), "no" | "false" | "0")
}

// ── playerctl plumbing ──────────────────────────────────────────────────────

/// Run playerctl, scoped to the current player when known.
fn pctl(args: &[&str]) -> host::ProcessOutput {
    let player = PLAYER.with(|p| p.borrow().clone());
    let mut full: Vec<String> = Vec::new();
    if !player.is_empty() {
        full.push("-p".into());
        full.push(player);
    }
    full.extend(args.iter().map(|s| s.to_string()));
    host::run("playerctl", &full)
}

fn ensure_started() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    let script = format!(": {TICKER_MARK}; while :; do printf '.\\n'; sleep 1; done");
    let id = host::process_start("sh", &["-c".to_string(), script]);
    TICKER.with(|t| *t.borrow_mut() = id);
}

/// Refresh the cached player name + shuffle/loop state (once per tick, so
/// render stays cheap and doesn't spawn playerctl on every event).
fn refresh_modes() {
    let m = host::media_now_playing();
    PLAYER.with(|p| *p.borrow_mut() = m.player.clone());
    if m.title.trim().is_empty() {
        return;
    }
    let sh = pctl(&["shuffle"]).stdout.trim().to_lowercase();
    SHUFFLE.with(|s| *s.borrow_mut() = sh == "on");
    let lp = pctl(&["loop"]).stdout.trim().to_lowercase();
    LOOP.with(|l| *l.borrow_mut() = lp);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn art_path(art_url: &str) -> Option<String> {
    if !show_art() {
        return None;
    }
    if let Some(p) = art_url.strip_prefix("file://") {
        // Strip a possible URL query/fragment; keep the plain path.
        return Some(p.to_string());
    }
    if art_url.starts_with('/') {
        return Some(art_url.to_string());
    }
    None
}

// ── UI ──────────────────────────────────────────────────────────────────────

fn art_view(m: &host::MediaInfo) -> El {
    match art_path(&m.art_url) {
        Some(path) => El::image(path)
            .prop("fit", "cover")
            .prop("width", "220")
            .prop("height", "220")
            .halign("center")
            .class("mplayer-art"),
        None => El::image("audio-x-generic-symbolic")
            .prop("width", "96")
            .prop("height", "96")
            .halign("center")
            .valign("center"),
    }
}

fn progress_row(m: &host::MediaInfo) -> El {
    let len = m.length_ms.max(1);
    let frac = (m.position_ms as f64 / len as f64).clamp(0.0, 1.0);
    El::vbox(vec![
        El::progress(frac).class("mplayer-progress"),
        El::hbox(vec![
            El::label(fmt_time(m.position_ms)).class("dim-label").halign("start").hexpand(true),
            El::label(fmt_time(m.length_ms)).class("dim-label").halign("end"),
        ]),
    ])
    .spacing(4)
}

fn controls_row(m: &host::MediaInfo) -> El {
    let playing = m.status == "playing";
    let shuffle_on = SHUFFLE.with(|s| *s.borrow());
    let loop_mode = LOOP.with(|l| l.borrow().clone());
    let loop_on = loop_mode == "track" || loop_mode == "playlist";
    let loop_icon = if loop_mode == "track" {
        "media-playlist-repeat-song-symbolic"
    } else {
        "media-playlist-repeat-symbolic"
    };

    let toggle = |id: &str, icon: &str, on: bool| {
        let class = if on {
            "plugin-panel-action mplayer-toggle-on"
        } else {
            "plugin-panel-action"
        };
        El::button(id.to_string(), "").prop("icon", icon).class(class)
    };

    El::hbox(vec![
        toggle("shuffle", "media-playlist-shuffle-symbolic", shuffle_on),
        El::button("prev", "")
            .prop("icon", "media-skip-backward-symbolic")
            .class("plugin-panel-action"),
        El::button("playpause", "")
            .prop(
                "icon",
                if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" },
            )
            .class("plugin-action plugin-action-primary mplayer-playpause"),
        El::button("next", "")
            .prop("icon", "media-skip-forward-symbolic")
            .class("plugin-panel-action"),
        toggle("loop", loop_icon, loop_on),
    ])
    .spacing(12)
    .halign("center")
}

fn seek_row() -> El {
    let step = seek_step();
    El::hbox(vec![
        El::button("seek-back", format!("{step}s"))
            .prop("icon", "media-seek-backward-symbolic")
            .class("plugin-chip"),
        El::button("seek-fwd", format!("{step}s"))
            .prop("icon", "media-seek-forward-symbolic")
            .class("plugin-chip"),
    ])
    .spacing(8)
    .halign("center")
}

fn view_tree() -> El {
    ensure_started();
    let m = host::media_now_playing();
    PLAYER.with(|p| *p.borrow_mut() = m.player.clone());

    if m.title.trim().is_empty() {
        return El::vbox(vec![
            El::image("audio-x-generic-symbolic")
                .prop("width", "72")
                .prop("height", "72")
                .halign("center"),
            El::label("Nothing playing").class("label-large-bold").halign("center"),
            El::label("Start a track in any media player.").class("dim-label").halign("center"),
        ])
        .spacing(12)
        .valign("center")
        .vexpand(true)
        .class("plugin-panel-large");
    }

    let meta = vec![
        El::label(m.title.clone()).class("label-large-bold").halign("center"),
        El::label(m.artist.clone()).class("label-medium-bold").halign("center"),
        El::label(if m.album.trim().is_empty() { String::new() } else { m.album.clone() })
            .class("dim-label")
            .halign("center"),
    ];

    // Cover + track info fade when the player is paused/stopped (matching the
    // built-in media pill); the transport stays full-strength so it's clearly
    // actionable.
    let head_class = if m.status == "playing" {
        "mplayer-head"
    } else {
        "mplayer-head mplayer-paused"
    };
    let head = El::vbox(vec![art_view(&m), El::vbox(meta).spacing(4)])
        .spacing(12)
        .class(head_class);

    El::vbox(vec![
        head,
        progress_row(&m),
        controls_row(&m),
        seek_row(),
    ])
    .spacing(16)
    .class("plugin-panel-large")
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Player;

impl Component for Player {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => {
                let step = seek_step().to_string();
                match ev.id.as_str() {
                    "playpause" => {
                        pctl(&["play-pause"]);
                    }
                    "prev" => {
                        pctl(&["previous"]);
                    }
                    "next" => {
                        pctl(&["next"]);
                    }
                    "seek-back" => {
                        pctl(&["position", &format!("{step}-")]);
                    }
                    "seek-fwd" => {
                        pctl(&["position", &format!("{step}+")]);
                    }
                    "shuffle" => {
                        pctl(&["shuffle", "toggle"]);
                        refresh_modes();
                    }
                    "loop" => {
                        // Cycle None → Playlist → Track → None.
                        let next = match LOOP.with(|l| l.borrow().clone()).as_str() {
                            "none" | "" => "Playlist",
                            "playlist" => "Track",
                            _ => "None",
                        };
                        pctl(&["loop", next]);
                        refresh_modes();
                    }
                    _ => {}
                }
            }
            EventKind::StreamChunk => {
                if TICKER.with(|t| *t.borrow() == ev.id) {
                    refresh_modes();
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Player);
