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

/// Which lrclib endpoint the in-flight request is hitting. `get` is the exact
/// match (artist+title+album+duration); `search` is the fuzzy fallback.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Get,
    Search,
}

thread_local! {
    static TICKER: RefCell<String> = const { RefCell::new(String::new()) };
    static FETCH_REQ: RefCell<String> = const { RefCell::new(String::new()) };
    static FETCH_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    static STAGE: RefCell<Stage> = const { RefCell::new(Stage::Get) };
    /// Cleaned (artist, title) the current fetch is for — used for the search
    /// fallback and for caching under a stable key.
    static QUERY: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
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

// ── Track-title cleanup (YouTube etc.) ──────────────────────────────────────

/// Strip the noise browsers/YouTube bake into MPRIS titles so lrclib can match:
/// bracketed tags ((Official Video), [HD], …), "feat./ft. …", a trailing
/// " - Topic", and "Artist - Title" when no separate artist is given.
fn clean_track(title: &str, artist: &str) -> (String, String) {
    let noise = [
        "official", "video", "audio", "lyric", "lyrics", "visualizer", "mv",
        "hd", "4k", "remaster", "remastered", "live", "explicit", "hq",
        "music video", "color coded", "performance",
    ];
    // Drop bracketed groups whose contents look like noise.
    let mut t = String::new();
    let mut depth = 0i32;
    let mut buf = String::new();
    for ch in title.chars() {
        match ch {
            '(' | '[' => {
                if depth == 0 {
                    buf.clear();
                }
                depth += 1;
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let low = buf.to_lowercase();
                    if !noise.iter().any(|n| low.contains(n)) {
                        // Keep non-noise parentheticals (e.g. "(Acoustic)").
                        t.push('(');
                        t.push_str(&buf);
                        t.push(')');
                    }
                }
            }
            _ if depth > 0 => buf.push(ch),
            _ => t.push(ch),
        }
    }
    // "feat. …" / "ft. …" → drop to end-of-segment.
    for sep in [" feat.", " feat ", " ft.", " ft ", " featuring "] {
        if let Some(idx) = t.to_lowercase().find(sep.trim_end()) {
            if t[..idx].to_lowercase().ends_with(sep.trim_end().trim_start())
                || t.to_lowercase()[idx..].starts_with(sep.trim())
            {
                t.truncate(idx);
            }
        }
    }
    let mut t = t.trim().trim_end_matches('-').trim().to_string();
    let mut a = artist.trim().trim_end_matches(" - Topic").trim().to_string();

    // "Artist - Title" with no separate artist → split.
    if a.is_empty() {
        if let Some((left, right)) = t.split_once(" - ") {
            a = left.trim().to_string();
            t = right.trim().to_string();
        }
    }
    (t, a)
}

// ── Fetch lifecycle ───────────────────────────────────────────────────────

fn ua() -> (String, String) {
    (
        "user-agent".into(),
        "margo-musiclyrics/0.1 (https://github.com/kenanpelit/margo-plugins)".into(),
    )
}

/// Begin loading lyrics for the given track: cache first, then lrclib `/get`,
/// then (on a miss) lrclib `/search`.
fn start_load(artist: &str, title: &str, album: &str, dur_secs: u64, force: bool) {
    let (title, artist) = clean_track(title, artist);
    LINES.with(|l| l.borrow_mut().clear());
    PLAIN.with(|p| p.borrow_mut().clear());
    FETCH_BUF.with(|b| b.borrow_mut().clear());
    STATUS.with(|s| *s.borrow_mut() = Status::Fetching);
    QUERY.with(|q| *q.borrow_mut() = (artist.clone(), title.clone()));

    if title.is_empty() {
        STATUS.with(|s| *s.borrow_mut() = Status::Idle);
        return;
    }

    // Cache hit?
    if !force && cache_enabled() {
        if let Ok(bytes) = host::read_file(&cache_key(&artist, &title)) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                apply_payload(&v);
                return;
            }
        }
    }

    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}",
        url_encode(&title),
    );
    if !artist.is_empty() {
        url.push_str(&format!("&artist_name={}", url_encode(&artist)));
    }
    if !album.is_empty() {
        url.push_str(&format!("&album_name={}", url_encode(album)));
    }
    if dur_secs > 0 {
        url.push_str(&format!("&duration={dur_secs}"));
    }
    STAGE.with(|s| *s.borrow_mut() = Stage::Get);
    let id = host::http_start(&host::HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![ua()],
        body: String::new(),
    });
    FETCH_REQ.with(|f| *f.borrow_mut() = id);
}

/// Kick off the fuzzy `/search` fallback for the current QUERY.
fn start_search() {
    let (artist, title) = QUERY.with(|q| q.borrow().clone());
    FETCH_BUF.with(|b| b.borrow_mut().clear());
    let mut url = format!("https://lrclib.net/api/search?track_name={}", url_encode(&title));
    if !artist.is_empty() {
        url.push_str(&format!("&artist_name={}", url_encode(&artist)));
    }
    STAGE.with(|s| *s.borrow_mut() = Stage::Search);
    let id = host::http_start(&host::HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![ua()],
        body: String::new(),
    });
    FETCH_REQ.with(|f| *f.borrow_mut() = id);
}

fn finish_fetch() {
    let body = FETCH_BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
    FETCH_REQ.with(|f| f.borrow_mut().clear());
    let stage = STAGE.with(|s| *s.borrow());
    let parsed = serde_json::from_str::<serde_json::Value>(&body);

    match stage {
        Stage::Get => match parsed {
            Ok(v) if v.get("syncedLyrics").is_some() || v.get("plainLyrics").is_some() => {
                apply_payload(&v);
                cache_current(&body);
            }
            // Exact match missed — try the fuzzy search before giving up.
            _ => start_search(),
        },
        Stage::Search => match parsed {
            Ok(serde_json::Value::Array(items)) => {
                // Prefer the first result that actually has synced lyrics.
                let best = items
                    .iter()
                    .find(|it| it["syncedLyrics"].as_str().map(|s| !s.is_empty()).unwrap_or(false))
                    .or_else(|| items.first());
                match best {
                    Some(v) => {
                        apply_payload(v);
                        cache_current(&v.to_string());
                    }
                    None => STATUS.with(|s| *s.borrow_mut() = Status::NotFound),
                }
            }
            Ok(_) => STATUS.with(|s| *s.borrow_mut() = Status::NotFound),
            Err(_) => STATUS.with(|s| *s.borrow_mut() = Status::Error),
        },
    }
}

fn cache_current(body: &str) {
    if cache_enabled() && STATUS.with(|s| *s.borrow() != Status::NotFound) {
        let (artist, title) = QUERY.with(|q| q.borrow().clone());
        let _ = host::write_file(&cache_key(&artist, &title), body.as_bytes());
    }
}

// ── Per-tick: detect track change, refresh pill line ────────────────────────

fn track_key(artist: &str, title: &str) -> String {
    format!("{artist}|{title}")
}

/// Detect a track change and (re)load lyrics. Idempotent — guarded by the raw
/// MPRIS key — so it's safe to call from both the 1 Hz tick and from `view`
/// (the latter means lyrics start loading the instant the panel opens, without
/// waiting up to a second for the first heartbeat).
fn sync_track(m: &host::MediaInfo) {
    if m.title.trim().is_empty() {
        if STATUS.with(|s| *s.borrow() != Status::Idle) {
            STATUS.with(|s| *s.borrow_mut() = Status::Idle);
            LINES.with(|l| l.borrow_mut().clear());
            PLAIN.with(|p| p.borrow_mut().clear());
            TRACK_KEY.with(|t| t.borrow_mut().clear());
        }
        return;
    }
    let key = track_key(&m.artist, &m.title);
    if key != TRACK_KEY.with(|t| t.borrow().clone()) {
        TRACK_KEY.with(|t| *t.borrow_mut() = key);
        start_load(&m.artist, &m.title, &m.album, m.length_ms / 1000, false);
    }
}

fn on_tick() {
    let m = host::media_now_playing();
    sync_track(&m);
    if m.title.trim().is_empty() {
        write_pill_line("");
    } else {
        write_pill_line(&current_line_text(m.position_ms as i64 + offset_ms()).unwrap_or_default());
    }
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
        .spacing(4)
        .hexpand(true),
        El::button("refresh", "")
            .prop("icon", "view-refresh-symbolic")
            .class("plugin-panel-action"),
    ])
    .class("plugin-panel-header")
    .spacing(12)
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
    El::scroll(rows).class("plugin-list").vexpand(true).spacing(8)
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
    // Start loading immediately on open (don't wait for the first heartbeat).
    sync_track(&m);
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
                    finish_fetch();
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Lyrics);
