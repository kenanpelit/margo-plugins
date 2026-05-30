//! Model Usage — AI assistant usage stats (Claude Code + OpenRouter).
//!
//! Ports the noctalia `model-usage` plugin to margo's WASM tier following
//! the §12 panel archetype in mshell-frame/DESIGN.md: leading-icon header
//! with a circular refresh action, pill-capsule segmented tabs, calm
//! surface-container cards inside, and tabular bar-rows for the recent
//! activity chart.
//!
//! Two providers in v1 (Codex / Copilot / Gemini / Zen are queued for the
//! next iteration once their auth surface is settled).

use mplugin_sdk::{
    export_component, host, host::HttpRequest, Component, El, Event, EventKind,
};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static TAB: RefCell<String> = RefCell::new("claude".to_string());
    static CLAUDE: RefCell<Option<ClaudeStats>> = const { RefCell::new(None) };
    static CLAUDE_ERR: RefCell<String> = const { RefCell::new(String::new()) };
    static OPENROUTER: RefCell<Option<OpenRouterStats>> = const { RefCell::new(None) };
    static OPENROUTER_ERR: RefCell<String> = const { RefCell::new(String::new()) };
    static INITED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Claude provider ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct RawClaudeStats {
    #[serde(rename = "dailyActivity", default)]
    daily_activity: Vec<RawDailyActivity>,
    #[serde(rename = "dailyModelTokens", default)]
    daily_model_tokens: Vec<RawDailyModelTokens>,
    #[serde(rename = "lastComputedDate", default)]
    last_computed_date: String,
}

#[derive(Deserialize, Default, Clone)]
struct RawDailyActivity {
    #[serde(default)]
    date: String,
    #[serde(rename = "messageCount", default)]
    message_count: i64,
    #[serde(rename = "sessionCount", default)]
    session_count: i64,
    #[serde(rename = "toolCallCount", default)]
    tool_call_count: i64,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)] // `date` preserved for round-trip / future per-day filtering.
struct RawDailyModelTokens {
    #[serde(default)]
    date: String,
    #[serde(rename = "tokensByModel", default)]
    tokens_by_model: BTreeMap<String, i64>,
}

#[derive(Clone)]
struct ClaudeStats {
    today: RawDailyActivity,
    total_messages: i64,
    total_sessions: i64,
    total_tool_calls: i64,
    days: Vec<RawDailyActivity>, // last 14, oldest-first
    by_model: BTreeMap<String, i64>,
    last_computed: String,
}

fn today_ymd() -> String {
    let out = host::run("date", &["+%Y-%m-%d".to_string()]);
    out.stdout.trim().to_string()
}

fn fetch_claude() {
    let out = host::run(
        "sh",
        &["-c".into(), "cat ~/.claude/stats-cache.json".into()],
    );
    if out.code != 0 {
        CLAUDE_ERR.with(|e| {
            *e.borrow_mut() =
                "stats-cache.json not found at ~/.claude/. Open a Claude Code session to seed it."
                    .to_string();
        });
        CLAUDE.with(|c| *c.borrow_mut() = None);
        return;
    }
    let raw: RawClaudeStats = match serde_json::from_str(&out.stdout) {
        Ok(r) => r,
        Err(e) => {
            CLAUDE_ERR.with(|err| *err.borrow_mut() = format!("parse error: {e}"));
            CLAUDE.with(|c| *c.borrow_mut() = None);
            return;
        }
    };
    let today_str = today_ymd();
    let today = raw
        .daily_activity
        .iter()
        .find(|d| d.date == today_str)
        .cloned()
        .unwrap_or_default();
    let total_messages: i64 = raw.daily_activity.iter().map(|d| d.message_count).sum();
    let total_sessions: i64 = raw.daily_activity.iter().map(|d| d.session_count).sum();
    let total_tool_calls: i64 = raw
        .daily_activity
        .iter()
        .map(|d| d.tool_call_count)
        .sum();
    let mut days = raw.daily_activity.clone();
    days.sort_by(|a, b| a.date.cmp(&b.date));
    let mut days = days.into_iter().rev().take(14).collect::<Vec<_>>();
    days.reverse();
    let mut by_model: BTreeMap<String, i64> = BTreeMap::new();
    for d in &raw.daily_model_tokens {
        for (model, n) in &d.tokens_by_model {
            *by_model.entry(model.clone()).or_default() += n;
        }
    }
    CLAUDE_ERR.with(|e| e.borrow_mut().clear());
    CLAUDE.with(|c| {
        *c.borrow_mut() = Some(ClaudeStats {
            today,
            total_messages,
            total_sessions,
            total_tool_calls,
            days,
            by_model,
            last_computed: raw.last_computed_date,
        });
    });
}

// ── OpenRouter provider ──────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct OpenRouterResponse {
    data: Option<OpenRouterData>,
}

#[derive(Deserialize, Default)]
struct OpenRouterData {
    #[serde(default)]
    label: String,
    #[serde(default)]
    usage: f64,
    #[serde(default)]
    limit: Option<f64>,
    #[serde(default)]
    limit_remaining: Option<f64>,
    #[serde(default)]
    is_provisioning_key: bool,
}

#[derive(Clone)]
struct OpenRouterStats {
    label: String,
    usage_usd: f64,
    limit_usd: Option<f64>,
    remaining_usd: Option<f64>,
    is_provisioning: bool,
}

fn fetch_openrouter() {
    let api_key = host::get_setting("openrouter_api_key");
    if api_key.trim().is_empty() {
        OPENROUTER_ERR.with(|e| {
            *e.borrow_mut() =
                "No API key set. Open Settings → Plugins → Model Usage and paste one.".to_string();
        });
        OPENROUTER.with(|c| *c.borrow_mut() = None);
        return;
    }
    let req = HttpRequest {
        method: "GET".into(),
        url: "https://openrouter.ai/api/v1/key".into(),
        headers: vec![("Authorization".into(), format!("Bearer {api_key}"))],
        body: String::new(),
    };
    match host::http(&req) {
        Ok(resp) if resp.status == 200 => {
            let parsed: OpenRouterResponse =
                serde_json::from_str(&resp.body).unwrap_or_default();
            let d = parsed.data.unwrap_or_default();
            OPENROUTER_ERR.with(|e| e.borrow_mut().clear());
            OPENROUTER.with(|c| {
                *c.borrow_mut() = Some(OpenRouterStats {
                    label: d.label,
                    usage_usd: d.usage,
                    limit_usd: d.limit,
                    remaining_usd: d.limit_remaining,
                    is_provisioning: d.is_provisioning_key,
                });
            });
        }
        Ok(resp) => {
            OPENROUTER_ERR.with(|e| {
                *e.borrow_mut() = format!("HTTP {} from /api/v1/key: {}", resp.status, resp.body);
            });
            OPENROUTER.with(|c| *c.borrow_mut() = None);
        }
        Err(e) => {
            OPENROUTER_ERR.with(|err| *err.borrow_mut() = format!("request failed: {e}"));
            OPENROUTER.with(|c| *c.borrow_mut() = None);
        }
    }
}

// ── Rendering helpers ────────────────────────────────────────────────────

fn fmt_num(n: i64) -> String {
    if n < 0 {
        return n.to_string();
    }
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// `[icon] Title                              ( ⟳ )` — the §12 panel header.
/// `action_id` is the id sent on the refresh-button click.
fn panel_header(icon: &str, title: &str, action_id: &str) -> El {
    El::hbox(vec![
        El::image(icon),
        El::label(title).hexpand(true).halign("start"),
        El::button(action_id, "")
            .class("plugin-panel-action")
            .prop("icon", "view-refresh-symbolic"),
    ])
    .spacing(8)
    .class("plugin-panel-header")
}

/// Tonal stat tile (label · value · hint).
fn stat_tile(label: &str, value: String, hint: Option<String>) -> El {
    let mut children = vec![
        El::label(label).class("plugin-stat-label"),
        El::label(value).class("plugin-stat-value"),
    ];
    if let Some(h) = hint {
        children.push(El::label(h).class("plugin-stat-hint"));
    }
    El::vbox(children)
        .spacing(4)
        .padding(12)
        .class("plugin-stat")
        .hexpand(true)
}

/// One row of the recent-activity / models-by-token bar charts:
/// `[label  ████████      count]`.
fn bar_row(label: &str, count: i64, peak: i64) -> El {
    let frac = if peak > 0 {
        (count as f64 / peak as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    El::hbox(vec![
        El::label(label.to_string()).prop("width", "120"),
        El::progress(frac).hexpand(true),
        El::label(fmt_num(count))
            .halign("end")
            .prop("width", "60"),
    ])
    .spacing(12)
    .class("plugin-bar-row")
}

/// Section heading inside a pane.
fn section_title(text: &str) -> El {
    El::label(text)
        .class("label-medium-bold")
        .halign("start")
        .padding(4)
}

// ── Panes ────────────────────────────────────────────────────────────────

fn claude_pane() -> El {
    let err = CLAUDE_ERR.with(|e| e.borrow().clone());
    let header = panel_header(
        "applications-development-symbolic",
        "Claude Code",
        "refresh",
    );

    if !err.is_empty() {
        return El::vbox(vec![
            header,
            El::label(err).class("dim-label").halign("start"),
            El::button("refresh", "Try again")
                .class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(12)
        .with_id("claude");
    }

    let opt = CLAUDE.with(|c| c.borrow().clone());
    let Some(s) = opt else {
        return El::vbox(vec![
            header,
            El::label("Loading Claude stats…").class("dim-label").halign("start"),
        ])
        .padding(12)
        .spacing(12)
        .with_id("claude");
    };

    let mut children: Vec<El> = vec![
        header,
        // Compact 3-stat strip — DESIGN.md §0 calm density.
        El::grid(
            3,
            vec![
                stat_tile(
                    "Today prompts",
                    fmt_num(s.today.message_count),
                    Some(format!("{} sessions", s.today.session_count)),
                ),
                stat_tile(
                    "Lifetime prompts",
                    fmt_num(s.total_messages),
                    Some(format!("{} sessions", s.total_sessions)),
                ),
                stat_tile(
                    "Tools today",
                    fmt_num(s.today.tool_call_count),
                    Some(format!("{} total", s.total_tool_calls)),
                ),
            ],
        )
        .spacing(12),
        El::label(format!("Source · ~/.claude/stats-cache.json · last computed {}", s.last_computed))
            .class("dim-label")
            .halign("start")
            .padding(4),
    ];

    if !s.days.is_empty() {
        let peak = s.days.iter().map(|d| d.message_count).max().unwrap_or(0);
        children.push(section_title("Recent activity"));
        let rows: Vec<El> = s
            .days
            .iter()
            .map(|d| bar_row(&d.date, d.message_count, peak))
            .collect();
        children.push(El::vbox(rows).spacing(4).class("plugin-card"));
    }

    if !s.by_model.is_empty() {
        children.push(section_title("Tokens by model"));
        let peak = s.by_model.values().copied().max().unwrap_or(0);
        let mut models: Vec<(&String, &i64)> = s.by_model.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));
        let rows: Vec<El> = models
            .into_iter()
            .map(|(m, t)| bar_row(m, *t, peak))
            .collect();
        children.push(El::vbox(rows).spacing(4).class("plugin-card"));
    }

    El::vbox(children).spacing(16).with_id("claude")
}

fn openrouter_pane() -> El {
    let err = OPENROUTER_ERR.with(|e| e.borrow().clone());
    let enabled = host::get_setting("openrouter_enabled") == "true";
    let header = panel_header("network-server-symbolic", "OpenRouter", "refresh");

    if !enabled {
        return El::vbox(vec![
            header,
            El::label("Disabled. Enable it in Settings → Plugins → Model Usage.")
                .class("dim-label")
                .halign("start"),
        ])
        .padding(12)
        .spacing(12)
        .with_id("openrouter");
    }
    if !err.is_empty() {
        return El::vbox(vec![
            header,
            El::label(err).class("dim-label").halign("start"),
            El::button("refresh", "Try again")
                .class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(12)
        .with_id("openrouter");
    }

    let opt = OPENROUTER.with(|c| c.borrow().clone());
    let Some(s) = opt else {
        return El::vbox(vec![
            header,
            El::label("Tap the refresh icon to fetch your OpenRouter usage.")
                .class("dim-label")
                .halign("start"),
        ])
        .padding(12)
        .spacing(12)
        .with_id("openrouter");
    };

    let limit_text = match s.limit_usd {
        Some(l) if l > 0.0 => format!("${:.2} / ${:.2}", s.usage_usd, l),
        _ => format!("${:.2} · no limit set", s.usage_usd),
    };
    let used_fraction = match (s.usage_usd, s.limit_usd) {
        (u, Some(l)) if l > 0.0 => (u / l).clamp(0.0, 1.0),
        _ => 0.0,
    };

    let mut tiles = vec![
        stat_tile(
            "Spent",
            format!("${:.2}", s.usage_usd),
            Some(limit_text.clone()),
        ),
    ];
    if let Some(r) = s.remaining_usd {
        tiles.push(stat_tile(
            "Remaining",
            format!("${:.2}", r),
            None,
        ));
    }

    let key_label = if s.label.trim().is_empty() {
        "default".to_string()
    } else {
        s.label.clone()
    };
    let provisioning_hint = if s.is_provisioning {
        Some("Provisioning key".to_string())
    } else {
        None
    };
    tiles.push(stat_tile("Key", key_label, provisioning_hint));

    El::vbox(vec![
        header,
        El::grid(tiles.len() as u32, tiles).spacing(12),
        section_title("Spending"),
        El::vbox(vec![
            El::progress(used_fraction).hexpand(true),
            El::label(limit_text).class("dim-label"),
        ])
        .spacing(8)
        .padding(12)
        .class("plugin-card"),
    ])
    .spacing(16)
    .with_id("openrouter")
}

// ── Component ───────────────────────────────────────────────────────────

fn ensure_inited() {
    INITED.with(|i| {
        if *i.borrow() {
            return;
        }
        *i.borrow_mut() = true;
        let default = host::get_setting("active_tab");
        if !default.is_empty() {
            TAB.with(|t| *t.borrow_mut() = default);
        }
    });
}

fn refresh_for(tab: &str) {
    match tab {
        "claude" => {
            if host::get_setting("claude_enabled") != "false" {
                fetch_claude();
            }
        }
        "openrouter" => {
            if host::get_setting("openrouter_enabled") == "true" {
                fetch_openrouter();
            }
        }
        _ => {}
    }
}

struct ModelUsage;

fn view_tree() -> El {
    ensure_inited();
    let tab = TAB.with(|t| t.borrow().clone());
    let needs_fetch = match tab.as_str() {
        "claude" => CLAUDE.with(|c| c.borrow().is_none())
            && CLAUDE_ERR.with(|e| e.borrow().is_empty()),
        "openrouter" => OPENROUTER.with(|c| c.borrow().is_none())
            && OPENROUTER_ERR.with(|e| e.borrow().is_empty()),
        _ => false,
    };
    if needs_fetch {
        refresh_for(&tab);
    }
    // Pill-capsule segmented control (DESIGN.md §12).
    let tabs = El::hbox(vec![
        El::button("tab-claude", "Claude").class(if tab == "claude" {
            "plugin-segment plugin-segment-on plugin-expand"
        } else {
            "plugin-segment plugin-expand"
        }),
        El::button("tab-openrouter", "OpenRouter").class(if tab == "openrouter" {
            "plugin-segment plugin-segment-on plugin-expand"
        } else {
            "plugin-segment plugin-expand"
        }),
    ])
    .class("plugin-segment-bar");

    El::vbox(vec![
        tabs,
        El::stack(tab.as_str(), vec![claude_pane(), openrouter_pane()]),
    ])
    .spacing(16)
    .class("plugin-panel-large")
}

impl Component for ModelUsage {
    fn view() -> El {
        view_tree()
    }
    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => match ev.id.as_str() {
                "tab-claude" => TAB.with(|t| *t.borrow_mut() = "claude".to_string()),
                "tab-openrouter" => TAB.with(|t| *t.borrow_mut() = "openrouter".to_string()),
                "refresh" => {
                    let tab = TAB.with(|t| t.borrow().clone());
                    refresh_for(&tab);
                }
                _ => {}
            },
            EventKind::Keybind if ev.id == "open" => {
                let tab = TAB.with(|t| t.borrow().clone());
                refresh_for(&tab);
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(ModelUsage);
