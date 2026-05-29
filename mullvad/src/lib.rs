//! Mullvad VPN — a rich in-shell control panel, written with `mplugin-sdk`.
//! Ported from the noctalia-shell plugin and built up: live status, connect /
//! disconnect / reconnect, lockdown + auto-connect toggles, and a searchable
//! country list (with relay counts) you click to connect.
//!
//! Drives the `mullvad` CLI through the host `run` capability (the same trust
//! level as the declarative tier's shell commands).

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

struct Status {
    connected: bool,
    state: String,
    relay: String,
    location: String,
}

fn status() -> Status {
    let s = mullvad(&["status"]);
    let mut st = Status {
        connected: false,
        state: "Disconnected".into(),
        relay: String::new(),
        location: String::new(),
    };
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

struct Mullvad;

fn view_tree() -> El {
    let st = status();
    let search = SEARCH.with(|s| s.borrow().clone());
    COUNTRIES.with(|c| {
        if c.borrow().is_empty() {
            *c.borrow_mut() = load_countries();
        }
    });

    let hero = if st.connected {
        El::markdown(format!("**🔒 {}** — `{}`\n{}", st.state, st.relay, st.location))
            .class("plugin-hero plugin-hero-on")
    } else {
        let loc = if st.location.is_empty() {
            String::new()
        } else {
            format!("\n{}", st.location)
        };
        El::markdown(format!("**🔓 {}**{}", st.state, loc)).class("plugin-hero")
    };

    let conn = if st.connected {
        El::button("conn", "Disconnect").class("plugin-action plugin-action-danger plugin-expand")
    } else {
        El::button("conn", "Connect").class("plugin-action plugin-action-primary plugin-expand")
    };

    let toggle = |id: &str, label: &str, on: bool| {
        El::button(id, format!("{label}  ·  {}", if on { "On" } else { "Off" }))
            .class(if on {
                "plugin-toggle plugin-toggle-on"
            } else {
                "plugin-toggle"
            })
    };
    let lockdown = toggle("lockdown", "Lockdown mode", setting_on("lockdown-mode"));
    let autoconnect = toggle("autoconnect", "Auto-connect", setting_on("auto-connect"));

    let needle = search.to_lowercase();
    let rows: Vec<El> = COUNTRIES.with(|c| {
        c.borrow()
            .iter()
            .filter(|(name, code, _)| {
                needle.is_empty()
                    || name.to_lowercase().contains(&needle)
                    || code.contains(&needle)
            })
            .map(|(name, code, count)| {
                El::button(format!("c:{code}"), format!("{name}  ·  {count}")).class("plugin-row")
            })
            .collect()
    });

    El::vbox(vec![
        hero,
        El::hbox(vec![
            conn,
            El::button("reconnect", "Reconnect").class("plugin-action plugin-expand"),
        ])
        .class("plugin-action-row"),
        lockdown,
        autoconnect,
        El::entry("search", &search).class("plugin-search"),
        El::scroll(rows).with_id("countries").class("plugin-list"),
    ])
    .class("plugin-panel-body")
}

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
                "lockdown" => {
                    let next = if setting_on("lockdown-mode") { "off" } else { "on" };
                    mullvad(&["lockdown-mode", "set", next]);
                }
                "autoconnect" => {
                    let next = if setting_on("auto-connect") { "off" } else { "on" };
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
