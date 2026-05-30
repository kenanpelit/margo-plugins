//! musiclyrics — time-synced lyrics for the margo shell, written with the
//! margo plugin authoring SDK (`mplugin-sdk`).
//!
//! Reads whatever's playing over MPRIS (`media-now-playing`), fetches synced
//! lyrics from [lrclib.net](https://lrclib.net) (free, no auth), caches them in
//! the plugin's data dir, and highlights the current line live as the song
//! plays. The bar pill can mirror the current line.
//!
//! ## Driving the live highlight
//!
//! The WASM tier has no timer, and the host re-renders on every event, so a
//! detached 1 Hz "ticker" subprocess (`process-start`, tagged for `pkill`)
//! pulses once a second; each pulse re-reads the MPRIS position and re-renders,
//! moving the highlight. A second async stream (`http-start`) carries the
//! lrclib response; the two are told apart by their request id.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

const TICKER_MARK: &str = "margolyricstick";
const LINE_FILE: &str = "line.txt";

#[derive(Clone, PartialEq)]
enum Status {
    Idle,
    Fetching,
    Synced,
    Plain,
    Instrumental,
    NotFound,
    Error,
}

thread_local! {
    static TICKER: RefCell<String> = const { RefCell::new(String::new()) };
    static FETCH_REQ: RefCell<String> = const { RefCell::new(String::new()) };
    static FETCH_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    /// Synced lines: (timestamp_ms, text), sorted.
    static LINES: RefCell<Vec<(i64, String)>> = const { RefCell::new(Vec::new()) };
    /// Plain (untimed) lyrics, used when no synced lyrics exist.
    static PLAIN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// "artist|title" the loaded lyrics belong to (track-change detector).
    static TRACK_KEY: RefCell<String> = const { RefCell::new(String::new()) };
    static STATUS: RefCell<Status> = const { RefCell::new(Status::Idle) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Settings ──────────────────────────────────────────────────────────────

fn offset_ms() -> i64 {
    host::get_setting("offset_ms").trim().parse().unwrap_or(0)
}

fn cache_enabled() -> bool {
    !matches!(host::get_setting("cache").to_lowercase().as_str(), "no" | "false" | "0")
}

fn pill_line_enabled() -> bool {
    !matches!(host::get_setting("pill_line").to_lowercase().as_str(), "no" | "false" | "0")
}

// ── Ticker ──────────────────────────────────────────────────────────────────

fn ensure_started() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    // 1 Hz heartbeat — re-reads the MPRIS position each second to move the
    // highlight. Tagged so a future stop could pkill it; here it lives for the
    // panel's lifetime (also keeps the bar pill's line.txt fresh while closed).
    let script = format!(": {TICKER_MARK}; while :; do printf '.\\n'; sleep 1; done");
    let id = host::process_start("sh", &["-c".to_string(), script]);
    TICKER.with(|t| *t.borrow_mut() = id);
}

// ── URL + parsing ─────────────────────────────────────────────────────────

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn parse_stamp(tag: &str) -> Option<i64> {
    let (m, rest) = tag.split_once(':')?;
    let mins: i64 = m.trim().parse().ok()?;
    let (secs, frac) = rest.split_once('.').unwrap_or((rest, "0"));
    let secs: i64 = secs.trim().parse().ok()?;
    // Fractional part is hundredths/thousandths — pad/truncate to 3 digits.
    let mut f = frac.trim().to_string();
    while f.len() < 3 {
        f.push('0');
    }
    let frac_ms: i64 = f[..3].parse().ok()?;
    Some((mins * 60 + secs) * 1000 + frac_ms)
}

/// Parse an LRC blob into sorted (ms, text) lines. A single source line may
/// carry several timestamps; each becomes its own entry.
fn parse_lrc(src: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for line in src.lines() {
        let mut rest = line;
        let mut stamps: Vec<i64> = Vec::new();
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            if let Some(ms) = parse_stamp(&rest[1..end]) {
                stamps.push(ms);
            }
            rest = &rest[end + 1..];
        }
        if stamps.is_empty() {
            continue;
        }
        let text = rest.trim().to_string();
        for ms in stamps {
            out.push((ms, text.clone()));
        }
    }
    out.sort_by_key(|(ms, _)| *ms);
    out
}

/// Index of the line that should be highlighted at `pos` ms (last line whose
/// timestamp is ≤ pos). `None` before the first line.
fn current_index(lines: &[(i64, String)], pos: i64) -> Option<usize> {
    if lines.is_empty() || pos < lines[0].0 {
        return None;
    }
    let mut idx = 0;
    for (i, (ms, _)) in lines.iter().enumerate() {
        if *ms <= pos {
            idx = i;
        } else {
            break;
        }
    }
    Some(idx)
}

// ── Cache ────────────────────────────────────────────────────────────────────

fn cache_key(artist: &str, title: &str) -> String {
    let raw = format!("{artist}-{title}").to_lowercase();
    let mut k: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    k.truncate(120);
    format!("cache/{k}.json")
}

/// Apply a parsed lrclib payload (or cached copy) to state.
fn apply_payload(v: &serde_json::Value) {
    let synced = v["syncedLyrics"].as_str().unwrap_or("");
    let plain = v["plainLyrics"].as_str().unwrap_or("");
    let instrumental = v["instrumental"].as_bool().unwrap_or(false);

    if instrumental {
        LINES.with(|l| l.borrow_mut().clear());
        PLAIN.with(|p| p.borrow_mut().clear());
        STATUS.with(|s| *s.borrow_mut() = Status::Instrumental);
        return;
    }
    let lines = parse_lrc(synced);
    if !lines.is_empty() {
        LINES.with(|l| *l.borrow_mut() = lines);
        PLAIN.with(|p| p.borrow_mut().clear());
        STATUS.with(|s| *s.borrow_mut() = Status::Synced);
    } else if !plain.trim().is_empty() {
        let pl: Vec<String> = plain.lines().map(|l| l.trim().to_string()).collect();
        PLAIN.with(|p| *p.borrow_mut() = pl);
        LINES.with(|l| l.borrow_mut().clear());
        STATUS.with(|s| *s.borrow_mut() = Status::Plain);
    } else {
        STATUS.with(|s| *s.borrow_mut() = Status::NotFound);
    }
}

// ── Fetch lifecycle ───────────────────────────────────────────────────────

/// Begin loading lyrics for the given track: cache first, then lrclib.
fn start_load(artist: &str, title: &str, album: &str, dur_secs: u64, force: bool) {
    LINES.with(|l| l.borrow_mut().clear());
    PLAIN.with(|p| p.borrow_mut().clear());
    FETCH_BUF.with(|b| b.borrow_mut().clear());
    STATUS.with(|s| *s.borrow_mut() = Status::Fetching);

    // Cache hit?
    if !force && cache_enabled() {
        if let Ok(bytes) = host::read_file(&cache_key(artist, title)) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                apply_payload(&v);
                return;
            }
        }
    }

    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        url_encode(artist),
        url_encode(title),
    );
    if !album.is_empty() {
        url.push_str(&format!("&album_name={}", url_encode(album)));
    }
    if dur_secs > 0 {
        url.push_str(&format!("&duration={dur_secs}"));
    }
    let id = host::http_start(&host::HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![(
            "user-agent".into(),
            "margo-musiclyrics/0.1 (https://github.com/kenanpelit/margo-plugins)".into(),
        )],
        body: String::new(),
    });
    FETCH_REQ.with(|f| *f.borrow_mut() = id);
}

fn finish_fetch(artist: &str, title: &str) {
    let body = FETCH_BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
    FETCH_REQ.with(|f| f.borrow_mut().clear());
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) if v.get("syncedLyrics").is_some() || v.get("plainLyrics").is_some() => {
            apply_payload(&v);
            // Cache the raw payload for next time.
            if cache_enabled() && STATUS.with(|s| *s.borrow() != Status::NotFound) {
                let _ = host::write_file(&cache_key(artist, title), &body.into_bytes());
            }
        }
        Ok(_) => STATUS.with(|s| *s.borrow_mut() = Status::NotFound),
        Err(_) => STATUS.with(|s| *s.borrow_mut() = Status::Error),
    }
}

// ── Per-tick: detect track change, refresh pill line ────────────────────────

fn track_key(artist: &str, title: &str) -> String {
    format!("{artist}|{title}")
}

fn on_tick() {
    let m = host::media_now_playing();
    if m.title.trim().is_empty() {
        // Nothing playing — clear state + pill line.
        if STATUS.with(|s| *s.borrow() != Status::Idle) {
            STATUS.with(|s| *s.borrow_mut() = Status::Idle);
            LINES.with(|l| l.borrow_mut().clear());
            PLAIN.with(|p| p.borrow_mut().clear());
            TRACK_KEY.with(|t| t.borrow_mut().clear());
        }
        write_pill_line("");
        return;
    }
    let key = track_key(&m.artist, &m.title);
    if key != TRACK_KEY.with(|t| t.borrow().clone()) {
        TRACK_KEY.with(|t| *t.borrow_mut() = key);
        start_load(&m.artist, &m.title, &m.album, m.length_ms / 1000, false);
    }
    // Refresh the bar pill's current line.
    write_pill_line(&current_line_text(m.position_ms as i64 + offset_ms()).unwrap_or_default());
}

fn current_line_text(pos: i64) -> Option<String> {
    LINES.with(|l| {
        let lines = l.borrow();
        current_index(&lines, pos).map(|i| lines[i].1.clone())
    })
}

fn write_pill_line(text: &str) {
    if pill_line_enabled() {
        let _ = host::write_file(LINE_FILE, text.as_bytes());
    }
}

// ── UI ──────────────────────────────────────────────────────────────────────

fn header(m: &host::MediaInfo) -> El {
    let (title, subtitle) = if m.title.trim().is_empty() {
        ("No music playing".to_string(), "Start a track to see its lyrics".to_string())
    } else {
        (m.title.clone(), m.artist.clone())
    };
    El::hbox(vec![
        El::image("audio-x-generic-symbolic"),
        El::vbox(vec![
            El::label(title).class("label-large-bold").halign("start"),
            El::label(subtitle).class("dim-label").halign("start"),
        ])
        .spacing(2)
        .hexpand(true),
        El::button("refresh", "")
            .prop("icon", "view-refresh-symbolic")
            .class("plugin-panel-action"),
    ])
    .class("plugin-panel-header")
    .spacing(10)
}

fn status_note() -> Option<El> {
    let s = STATUS.with(|s| s.borrow().clone());
    let (text, ok) = match s {
        Status::Fetching => ("Searching lyrics…", false),
        Status::Synced => ("Synced · lrclib.net", true),
        Status::Plain => ("Unsynced lyrics · lrclib.net", true),
        Status::Instrumental => ("Instrumental", false),
        Status::NotFound => ("No lyrics found", false),
        Status::Error => ("Couldn't reach lrclib.net", false),
        Status::Idle => return None,
    };
    let class = if ok {
        "plugin-status-badge plugin-status-ok"
    } else {
        "plugin-status-badge"
    };
    Some(El::label(text).class(class).halign("center"))
}

fn synced_view(pos: i64) -> El {
    let lines = LINES.with(|l| l.borrow().clone());
    let cur = current_index(&lines, pos);
    // Window the lines around the active one so it's always visible without
    // programmatic scrolling (the renderer can't auto-scroll for us).
    let center = cur.unwrap_or(0);
    let start = center.saturating_sub(2);
    let end = (center + 7).min(lines.len());
    let rows: Vec<El> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, (_, text))| {
            let idx = start + i;
            let display = if text.is_empty() { "♪".to_string() } else { text.clone() };
            if Some(idx) == cur {
                El::label(display)
                    .class("lyrics-line lyrics-line-active")
                    .halign("center")
            } else {
                El::label(display).class("lyrics-line dim-label").halign("center")
            }
        })
        .collect();
    El::vbox(rows).spacing(8).vexpand(true).valign("center")
}

fn plain_view() -> El {
    let plain = PLAIN.with(|p| p.borrow().clone());
    let rows: Vec<El> = plain
        .iter()
        .map(|l| {
            let display = if l.is_empty() { " ".to_string() } else { l.clone() };
            El::label(display).class("lyrics-line").halign("center")
        })
        .collect();
    El::scroll(rows).class("plugin-list").vexpand(true).spacing(6)
}

fn message(text: &str) -> El {
    El::label(text)
        .class("dim-label")
        .halign("center")
        .valign("center")
        .vexpand(true)
}

fn view_tree() -> El {
    ensure_started();
    let m = host::media_now_playing();
    let pos = m.position_ms as i64 + offset_ms();
    let status = STATUS.with(|s| s.borrow().clone());

    let body = match status {
        Status::Synced => synced_view(pos),
        Status::Plain => plain_view(),
        Status::Fetching => message("Searching lyrics…"),
        Status::Instrumental => message("This track is instrumental."),
        Status::NotFound => message("No lyrics found for this track."),
        Status::Error => message("Couldn't reach lrclib.net — check your connection."),
        Status::Idle => message("Nothing playing right now."),
    };

    let mut children = vec![header(&m)];
    if let Some(note) = status_note() {
        children.push(note);
    }
    children.push(El::separator());
    children.push(body);

    El::vbox(children).spacing(12).class("plugin-panel-large")
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Lyrics;

impl Component for Lyrics {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click if ev.id == "refresh" => {
                let m = host::media_now_playing();
                if !m.title.trim().is_empty() {
                    TRACK_KEY.with(|t| *t.borrow_mut() = track_key(&m.artist, &m.title));
                    start_load(&m.artist, &m.title, &m.album, m.length_ms / 1000, true);
                }
            }
            EventKind::StreamChunk => {
                if TICKER.with(|t| *t.borrow() == ev.id) {
                    on_tick();
                } else if FETCH_REQ.with(|f| *f.borrow() == ev.id) {
                    FETCH_BUF.with(|b| b.borrow_mut().push_str(&ev.value));
                }
            }
            EventKind::StreamEnd => {
                if FETCH_REQ.with(|f| *f.borrow() == ev.id) {
                    let m = host::media_now_playing();
                    finish_fetch(&m.artist, &m.title);
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Lyrics);
