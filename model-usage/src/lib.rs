//! Model Usage — AI assistant usage stats (Claude Code + OpenRouter).
//!
//! Ports the noctalia `model-usage` plugin to margo's WASM tier. Two
//! providers in v1 (Codex / Copilot / Gemini / Zen are queued for the
//! next iteration once their auth surface is settled).
//!
//! Architecture:
//! - Each provider's snapshot lives in a `thread_local!` state cell.
//! - `view()` reads the cells; it does *not* fetch — fetching is on demand
//!   from `update(ev)` via Refresh / tab clicks / opens.
//! - Settings (provider enable + API key) come through `host::get_setting`.
//!   The `openrouter_api_key` is `type = "secret"` so it lives in the
//!   system keyring; the bridge transparently maps it back here.
//! - The panel is a stack with one tab per provider; the inactive tab is
//!   never fetched until the user clicks it (so OpenRouter's HTTP request
//!   only happens when its tab is selected).

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
#[allow(dead_code)] // `date` is preserved for round-trip / future per-day filtering.
struct RawDailyModelTokens {
    #[serde(default)]
    date: String,
    #[serde(rename = "tokensByModel", default)]
    tokens_by_model: BTreeMap<String, i64>,
}

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
    // Tilde expansion needs a shell; the host's `run` calls exec directly.
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
    let days = days.into_iter().rev().take(14).collect::<Vec<_>>();
    let mut days = days;
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

fn stat_card(label: &str, value: String, hint: Option<String>) -> El {
    let mut children = vec![
        El::label(label).class("dim-label"),
        El::label(value).class("label-large-bold"),
    ];
    if let Some(h) = hint {
        children.push(El::label(h).class("dim-label"));
    }
    El::vbox(children)
        .padding(10)
        .class("plugin-row")
        .hexpand(true)
}

fn bar_chart_row(date: &str, count: i64, peak: i64) -> El {
    let frac = if peak > 0 {
        (count as f64 / peak as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    El::hbox(vec![
        El::label(date.to_string())
            .class("dim-label")
            .prop("width", "100"),
        El::progress(frac).hexpand(true),
        El::label(fmt_num(count)).halign("end").prop("width", "60"),
    ])
    .spacing(8)
    .padding(2)
}

// ── Panes ────────────────────────────────────────────────────────────────

fn claude_pane() -> El {
    let err = CLAUDE_ERR.with(|e| e.borrow().clone());
    if !err.is_empty() {
        return El::vbox(vec![
            El::markdown(format!("**Claude Code**\n{err}")).class("plugin-hero"),
            El::button("refresh", "Try again")
                .class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(10)
        .with_id("claude");
    }
    let opt = CLAUDE.with(|c| c.borrow().as_ref().map(snapshot_for_claude));
    let Some(s) = opt else {
        return El::vbox(vec![
            El::label("Loading Claude stats…").class("dim-label"),
            El::button("refresh", "Reload").class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(10)
        .with_id("claude");
    };

    let mut children: Vec<El> = vec![
        El::markdown(format!(
            "**Claude Code** — {} prompts today · last updated `{}`",
            s.today.message_count, s.last_computed
        ))
        .class("plugin-hero plugin-hero-on"),
        El::grid(
            3,
            vec![
                stat_card("Today", fmt_num(s.today.message_count), Some(format!("{} sessions", s.today.session_count))),
                stat_card("Lifetime", fmt_num(s.total_messages), Some(format!("{} sessions", s.total_sessions))),
                stat_card("Tools today", fmt_num(s.today.tool_call_count), Some(format!("{} total", s.total_tool_calls))),
            ],
        )
        .spacing(8),
    ];

    if !s.days.is_empty() {
        let peak = s.days.iter().map(|d| d.message_count).max().unwrap_or(0);
        children.push(El::separator());
        children.push(El::label("Recent activity").class("label-medium-bold"));
        let rows: Vec<El> = s
            .days
            .iter()
            .map(|d| bar_chart_row(&d.date, d.message_count, peak))
            .collect();
        children.push(El::vbox(rows).spacing(2));
    }

    if !s.by_model.is_empty() {
        children.push(El::separator());
        children.push(El::label("Tokens by model").class("label-medium-bold"));
        let peak = s.by_model.values().copied().max().unwrap_or(0);
        let mut models: Vec<(&String, &i64)> = s.by_model.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));
        for (model, tokens) in models {
            children.push(bar_chart_row(model, *tokens, peak));
        }
    }

    children.push(
        El::button("refresh", "Refresh")
            .class("plugin-action plugin-action-primary")
            .hexpand(true),
    );

    El::vbox(children).padding(12).spacing(8).with_id("claude")
}

fn snapshot_for_claude(s: &ClaudeStats) -> ClaudeStats {
    ClaudeStats {
        today: s.today.clone(),
        total_messages: s.total_messages,
        total_sessions: s.total_sessions,
        total_tool_calls: s.total_tool_calls,
        days: s.days.clone(),
        by_model: s.by_model.clone(),
        last_computed: s.last_computed.clone(),
    }
}

fn openrouter_pane() -> El {
    let err = OPENROUTER_ERR.with(|e| e.borrow().clone());
    let enabled = host::get_setting("openrouter_enabled") == "true";

    if !enabled {
        return El::vbox(vec![
            El::markdown("**OpenRouter**\nDisabled. Enable it in Settings → Plugins → Model Usage.")
                .class("plugin-hero"),
        ])
        .padding(12)
        .with_id("openrouter");
    }
    if !err.is_empty() {
        return El::vbox(vec![
            El::markdown(format!("**OpenRouter**\n{err}")).class("plugin-hero"),
            El::button("refresh", "Try again")
                .class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(10)
        .with_id("openrouter");
    }

    let opt = OPENROUTER.with(|c| {
        c.borrow().as_ref().map(|s| OpenRouterStats {
            label: s.label.clone(),
            usage_usd: s.usage_usd,
            limit_usd: s.limit_usd,
            remaining_usd: s.remaining_usd,
            is_provisioning: s.is_provisioning,
        })
    });
    let Some(s) = opt else {
        return El::vbox(vec![
            El::label("Tap Refresh to fetch your OpenRouter usage.").class("dim-label"),
            El::button("refresh", "Refresh").class("plugin-action plugin-action-primary"),
        ])
        .padding(12)
        .spacing(10)
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
    let remaining_text = s
        .remaining_usd
        .map(|r| format!("${:.2} remaining", r))
        .unwrap_or_else(|| "—".to_string());

    let label = if s.label.trim().is_empty() {
        "OpenRouter".to_string()
    } else {
        s.label.clone()
    };
    let provisioning = if s.is_provisioning {
        " · provisioning key"
    } else {
        ""
    };

    El::vbox(vec![
        El::markdown(format!("**OpenRouter — {label}**\n{limit_text}{provisioning}"))
            .class("plugin-hero plugin-hero-on"),
        El::label("Spending").class("label-medium-bold"),
        El::progress(used_fraction).hexpand(true),
        El::label(remaining_text).class("dim-label"),
        El::separator(),
        El::button("refresh", "Refresh")
            .class("plugin-action plugin-action-primary")
            .hexpand(true),
    ])
    .padding(12)
    .spacing(8)
    .with_id("openrouter")
}

// ── Component ───────────────────────────────────────────────────────────

fn ensure_inited() {
    INITED.with(|i| {
        if *i.borrow() {
            return;
        }
        *i.borrow_mut() = true;
        // Pull the user's preferred default tab.
        let default = host::get_setting("active_tab");
        if !default.is_empty() {
            TAB.with(|t| *t.borrow_mut() = default);
        }
        // First fetch happens lazily; the user opens the panel + we fetch
        // for the visible tab immediately so they see data on open.
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
    // Fetch the active tab on demand if its cache is empty (so opening the
    // panel without manually hitting Refresh still shows data).
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
    El::vbox(vec![
        El::hbox(vec![
            El::button("tab-claude", "Claude").class(if tab == "claude" {
                "plugin-toggle plugin-toggle-on plugin-expand"
            } else {
                "plugin-toggle plugin-expand"
            }),
            El::button("tab-openrouter", "OpenRouter").class(if tab == "openrouter" {
                "plugin-toggle plugin-toggle-on plugin-expand"
            } else {
                "plugin-toggle plugin-expand"
            }),
        ])
        .spacing(6),
        El::stack(tab.as_str(), vec![claude_pane(), openrouter_pane()]),
    ])
    .spacing(10)
    .padding(8)
    .class("plugin-panel-body")
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
                // The shell already opened the panel; just refresh the active
                // tab so the freshest data is on screen.
                let tab = TAB.with(|t| t.borrow().clone());
                refresh_for(&tab);
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(ModelUsage);
