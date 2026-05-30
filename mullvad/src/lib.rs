//! Mullvad VPN — a rich in-shell control panel.
//!
//! Visual design inspired by the DankMaterialShell AdGuard VPN plugin
//! (large hero card, status badge, stat-tile grid, big primary action,
//! quick-action chip row, toggle cards with descriptions, location list
//! with relay-count chips) but recoloured through margo's matugen tokens
//! and built on the WASM tier's panel-archetype design language.
//!
//! Drives the `mullvad` CLI through the host `run` capability.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    static SEARCH: RefCell<String> = const { RefCell::new(String::new()) };
    /// Cached relay countries: (name, code, relay-count). Loaded once.
    static COUNTRIES: RefCell<Vec<(String, String, u32)>> = const { RefCell::new(Vec::new()) };
}

/// Run `mullvad <args>` and return stdout.
fn mullvad(args: &[&str]) -> String {
    let argv: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    host::run("mullvad", &argv).stdout
}

#[derive(Default, Clone)]
struct Status {
    connected: bool,
    state: String,
    relay: String,
    location: String,
    ipv4: String,
    country: String,
    city: String,
    tunnel_type: String,
}

fn status() -> Status {
    let s = mullvad(&["status"]);
    let mut st = Status::default();
    st.state = "Disconnected".into();
    for (i, line) in s.lines().enumerate() {
        let t = line.trim();
        if i == 0 {
            st.state = t.to_string();
            let lc = t.to_lowercase();
            st.connected = lc.starts_with("connected");
        }
        if let Some(r) = t.strip_prefix("Relay:") {
            st.relay = r.trim().to_string();
        }
        if let Some(l) = t.strip_prefix("Visible location:") {
            st.location = l.trim().to_string();
            // "City, Country. IPv4: 1.2.3.4" — split out the pieces.
            if let Some((before_ip, after_ip)) = l.split_once("IPv4:") {
                st.ipv4 = after_ip.trim().to_string();
                let trimmed = before_ip.trim().trim_end_matches('.').trim().to_string();
                if let Some((city, country)) = trimmed.split_once(',') {
                    st.city = city.trim().to_string();
                    st.country = country.trim().to_string();
                } else {
                    st.country = trimmed;
                }
            }
        }
        if let Some(p) = t.strip_prefix("Tunnel:") {
            st.tunnel_type = p.trim().to_string();
        }
    }
    st
}

/// A boolean setting whose `get` prints "… : on/off".
fn setting_on(subcommand: &str) -> bool {
    mullvad(&[subcommand, "get"])
        .to_lowercase()
        .trim()
        .ends_with("on")
}

fn account_expiry() -> String {
    let s = mullvad(&["account", "get"]);
    for line in s.lines() {
        let t = line.trim();
        if let Some(e) = t.strip_prefix("Expires at") {
            // Keep just the date (drop the verbose timestamp tail).
            let s = e.trim_start_matches(':').trim();
            return s.chars().take(10).collect();
        }
        if let Some(e) = t.strip_prefix("Expires:") {
            return e.trim().chars().take(10).collect();
        }
    }
    "—".into()
}

/// Parse `mullvad relay list`: top-level "Name (code)" lines are countries;
/// doubly-indented lines are relays — count them per country.
fn load_countries() -> Vec<(String, String, u32)> {
    let s = mullvad(&["relay", "list"]);
    let mut out: Vec<(String, String, u32)> = Vec::new();
    for line in s.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("\t\t") {
            if let Some(last) = out.last_mut() {
                last.2 += 1;
            }
        } else if !line.starts_with('\t') {
            if let Some(open) = line.rfind('(') {
                let name = line[..open].trim().to_string();
                let code = line[open + 1..].trim_end_matches(')').trim().to_string();
                if !name.is_empty() && !code.is_empty() {
                    out.push((name, code, 0));
                }
            }
        }
    }
    out
}

// ── Render helpers ───────────────────────────────────────────────────────

fn stat_tile(label: &str, value: String) -> El {
    // The value is short *text* (a country, city, relay id, IP) — not a big
    // dashboard number — so it uses the standard menu title size
    // (`label-medium-bold`, 16px) instead of `.plugin-stat-value` (26px),
    // which read oversized and out of step with the rest of the shell's menus.
    El::vbox(vec![
        El::label(label).class("plugin-stat-label"),
        El::label(value).class("label-medium-bold"),
    ])
    .spacing(4)
    .padding(12)
    .class("plugin-stat")
    .hexpand(true)
}

fn toggle_card(id: &str, title: &str, description: &str, on: bool) -> El {
    El::hbox(vec![
        El::vbox(vec![
            El::label(title)
                .class("label-medium-bold")
                .halign("start"),
            El::label(description)
                .class("dim-label")
                .halign("start"),
        ])
        .spacing(2)
        .hexpand(true),
        El::switch(id, on).valign("center"),
    ])
    .spacing(12)
    .padding(12)
    .class("plugin-card")
}

fn hero_card(st: &Status) -> El {
    if st.connected {
        let headline = if !st.country.is_empty() {
            format!("Connected to {}", st.country)
        } else {
            "Connected".to_string()
        };
        let subtitle = if !st.city.is_empty() {
            st.city.clone()
        } else {
            st.location.clone()
        };
        El::hbox(vec![
            El::image("network-vpn-symbolic"),
            El::vbox(vec![
                El::label(headline)
                    .class("label-large-bold")
                    .halign("start"),
                El::label(subtitle)
                    .class("dim-label")
                    .halign("start"),
            ])
            .spacing(2)
            .hexpand(true),
            if st.relay.is_empty() {
                El::label("—")
            } else {
                El::label(st.relay.clone())
            }
            .class("plugin-status-badge plugin-status-ok")
            .valign("center"),
        ])
        .spacing(12)
        .padding(14)
        .class("plugin-hero plugin-hero-on")
    } else {
        El::hbox(vec![
            El::image("network-vpn-disabled-symbolic"),
            El::vbox(vec![
                El::label("Not connected")
                    .class("label-large-bold")
                    .halign("start"),
                El::label("Hit Connect to bring the tunnel up.")
                    .class("dim-label")
                    .halign("start"),
            ])
            .spacing(2)
            .hexpand(true),
        ])
        .spacing(12)
        .padding(14)
        .class("plugin-hero")
    }
}

fn status_chip(label: &str, value: &str, ok: bool) -> El {
    let class = if ok {
        "plugin-status-badge plugin-status-ok"
    } else {
        "plugin-status-badge"
    };
    El::label(format!("{label}: {value}")).class(class)
}

fn view_tree() -> El {
    let st = status();
    let search = SEARCH.with(|s| s.borrow().clone());
    COUNTRIES.with(|c| {
        if c.borrow().is_empty() {
            *c.borrow_mut() = load_countries();
        }
    });

    let lockdown_on = setting_on("lockdown-mode");
    let autoconnect_on = setting_on("auto-connect");
    let expiry = account_expiry();

    // ── Header ──────────────────────────────────────────────────────
    let header = El::hbox(vec![
        El::image("network-vpn-symbolic"),
        El::label("Mullvad VPN").hexpand(true).halign("start"),
        if st.connected {
            El::label("Active").class("plugin-status-badge plugin-status-ok")
        } else {
            El::label("Inactive").class("plugin-status-badge")
        }
        .halign("end"),
        El::button("refresh", "")
            .class("plugin-panel-action")
            .prop("icon", "view-refresh-symbolic"),
    ])
    .spacing(8)
    .class("plugin-panel-header");

    // ── Status chips row (account + lockdown + autoconnect) ─────────
    let chips = El::hbox(vec![
        status_chip("Account expires", &expiry, !expiry.is_empty() && expiry != "—"),
        status_chip(
            "Lockdown",
            if lockdown_on { "on" } else { "off" },
            lockdown_on,
        ),
        status_chip(
            "Auto-connect",
            if autoconnect_on { "on" } else { "off" },
            autoconnect_on,
        ),
    ])
    .spacing(6);

    // ── Stat-tile grid ──────────────────────────────────────────────
    let stats = El::grid(
        2,
        vec![
            stat_tile(
                "Country",
                if st.country.is_empty() { "—".into() } else { st.country.clone() },
            ),
            stat_tile(
                "City",
                if st.city.is_empty() { "—".into() } else { st.city.clone() },
            ),
            stat_tile(
                "Relay",
                if st.relay.is_empty() { "—".into() } else { st.relay.clone() },
            ),
            stat_tile(
                "Public IPv4",
                if st.ipv4.is_empty() { "—".into() } else { st.ipv4.clone() },
            ),
        ],
    )
    .spacing(8);

    // ── Big primary action ──────────────────────────────────────────
    let (action_label, action_class) = if st.connected {
        ("Disconnect", "plugin-action plugin-action-danger plugin-expand")
    } else {
        ("Connect", "plugin-action plugin-action-primary plugin-expand")
    };

    // ── Quick-action chip row ───────────────────────────────────────
    let quick_row = El::hbox(vec![
        El::button("reconnect", "Reconnect").class("plugin-action plugin-expand"),
        El::button("refresh", "Refresh").class("plugin-action plugin-expand"),
    ])
    .spacing(8);

    // ── Toggle cards ────────────────────────────────────────────────
    let toggles = El::vbox(vec![
        toggle_card(
            "lockdown",
            "Lockdown mode",
            "Block all internet when the VPN drops.",
            lockdown_on,
        ),
        toggle_card(
            "autoconnect",
            "Auto-connect on startup",
            "Bring the tunnel up automatically when the daemon starts.",
            autoconnect_on,
        ),
    ])
    .spacing(8);

    // ── Locations section ───────────────────────────────────────────
    let needle = search.to_lowercase();
    let filtered: Vec<(String, String, u32)> = COUNTRIES.with(|c| {
        c.borrow()
            .iter()
            .filter(|(name, code, _)| {
                needle.is_empty()
                    || name.to_lowercase().contains(&needle)
                    || code.contains(&needle)
            })
            .cloned()
            .collect()
    });
    let total = COUNTRIES.with(|c| c.borrow().len());
    // GTK Buttons take a label string, not a child node — so build each row
    // as an hbox with a small "Connect" trailing button.
    let rows: Vec<El> = filtered
        .iter()
        .map(|(name, code, count)| {
            El::hbox(vec![
                El::label(format!("[{}]", code.to_uppercase()))
                    .class("plugin-key-pill")
                    .valign("center"),
                El::label(name.clone())
                    .halign("start")
                    .hexpand(true),
                El::label(if *count == 1 {
                    format!("{count} relay")
                } else {
                    format!("{count} relays")
                })
                .class("plugin-status-badge"),
                El::button(format!("c:{code}"), "Connect")
                    .class("plugin-action plugin-action-primary"),
            ])
            .spacing(10)
            .padding(8)
            .class("plugin-bar-row")
        })
        .collect();

    let count_label = if needle.is_empty() {
        format!("{} countries", total)
    } else {
        format!("{} / {}", filtered.len(), total)
    };
    let locations_header = El::hbox(vec![
        El::label("Locations")
            .class("label-medium-bold")
            .halign("start")
            .hexpand(true),
        El::label(count_label).class("plugin-status-badge"),
    ])
    .spacing(8);

    El::vbox(vec![
        header,
        chips,
        hero_card(&st),
        stats,
        El::button("conn", action_label).class(action_class),
        quick_row,
        El::separator(),
        toggles,
        El::separator(),
        locations_header,
        El::entry("search", &search)
            .class("plugin-search")
            .hexpand(true),
        El::scroll(rows)
            .with_id("countries")
            .class("plugin-list")
            .vexpand(true),
    ])
    .spacing(12)
    .class("plugin-panel-large")
}

struct Mullvad;

impl Component for Mullvad {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Submit if ev.id == "search" => {
                SEARCH.with(|s| *s.borrow_mut() = ev.value.clone());
            }
            EventKind::Click => match ev.id.as_str() {
                "conn" => {
                    if status().connected {
                        mullvad(&["disconnect"]);
                    } else {
                        mullvad(&["connect"]);
                    }
                }
                "reconnect" => {
                    mullvad(&["reconnect"]);
                }
                "refresh" => {
                    // Force-reload the country cache + status on the next view.
                    COUNTRIES.with(|c| c.borrow_mut().clear());
                }
                "lockdown" => {
                    // Switch toggle sends "true"/"false"; click button toggles current.
                    let want = !setting_on("lockdown-mode");
                    let next = if want { "on" } else { "off" };
                    mullvad(&["lockdown-mode", "set", next]);
                }
                "autoconnect" => {
                    let want = !setting_on("auto-connect");
                    let next = if want { "on" } else { "off" };
                    mullvad(&["auto-connect", "set", next]);
                }
                id if id.starts_with("c:") => {
                    let code = &id[2..];
                    mullvad(&["relay", "set", "location", code]);
                    mullvad(&["connect"]);
                }
                _ => {}
            },
            _ => {}
        }
        view_tree()
    }
}

export_component!(Mullvad);
