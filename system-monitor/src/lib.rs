//! System Monitor — live RAM / load / disk / uptime + battery + network in
//! one panel. Demonstrates `host::run` against /proc plus the typed
//! `system-state` capability.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};

struct SystemMonitor;

fn read(program: &str, args: &[&str]) -> String {
    let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    host::run(program, &argv).stdout
}

struct MemInfo {
    used_pct: u32,
    used_mib: u32,
    total_mib: u32,
}

fn mem_info() -> MemInfo {
    let s = read("cat", &["/proc/meminfo"]);
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in s.lines() {
        let mut iter = line.split_whitespace();
        let Some(key) = iter.next() else {
            continue;
        };
        let Some(val) = iter.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        match key {
            "MemTotal:" => total_kb = val,
            "MemAvailable:" => avail_kb = val,
            _ => {}
        }
    }
    let used_kb = total_kb.saturating_sub(avail_kb);
    MemInfo {
        used_pct: if total_kb > 0 {
            (used_kb * 100 / total_kb) as u32
        } else {
            0
        },
        used_mib: (used_kb / 1024) as u32,
        total_mib: (total_kb / 1024) as u32,
    }
}

struct LoadInfo {
    one: f64,
    five: f64,
    fifteen: f64,
}

fn load_info() -> LoadInfo {
    let s = read("cat", &["/proc/loadavg"]);
    let parts: Vec<&str> = s.split_whitespace().collect();
    LoadInfo {
        one: parts.first().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        five: parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0),
        fifteen: parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0),
    }
}

fn uptime_string() -> String {
    let s = read("cat", &["/proc/uptime"]);
    let secs = s
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

struct DiskInfo {
    used_pct: u32,
    used_gib: f64,
    total_gib: f64,
}

fn disk_info() -> DiskInfo {
    // `df -B1` keeps everything in bytes — convert to GiB once.
    let s = read("df", &["-B1", "--output=used,size", "/"]);
    let mut nums = s.split_whitespace().skip(2);
    let used: u64 = nums.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let total: u64 = nums.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    DiskInfo {
        used_pct: ((used as f64 / total.max(1) as f64) * 100.0).round() as u32,
        used_gib: used as f64 / 1024.0_f64.powi(3),
        total_gib: total as f64 / 1024.0_f64.powi(3),
    }
}

fn bar(label: &str, fraction: f64) -> El {
    El::vbox(vec![
        El::hbox(vec![
            El::label(label).hexpand(true),
            El::label(format!("{:.0}%", fraction * 100.0)).halign("end"),
        ]),
        El::progress(fraction.clamp(0.0, 1.0)).hexpand(true),
    ])
    .spacing(4)
    .padding(4)
}

fn view_tree() -> El {
    let mem = mem_info();
    let load = load_info();
    let disk = disk_info();
    let up = uptime_string();
    let sys = host::system_state();

    let battery = if sys.battery_pct == 255 {
        "—".to_string()
    } else {
        format!("{}% · {}", sys.battery_pct, sys.battery_status)
    };
    let network = match sys.network_kind.as_str() {
        "wifi" if !sys.network_ssid.is_empty() => format!("wifi · {}", sys.network_ssid),
        kind => kind.to_string(),
    };

    El::vbox(vec![
        El::markdown(format!(
            "**System Monitor**\nuptime: `{up}` · load: `{:.2} {:.2} {:.2}`",
            load.one, load.five, load.fifteen
        ))
        .class("plugin-hero"),
        bar(
            &format!("RAM · {} / {} MiB", mem.used_mib, mem.total_mib),
            mem.used_pct as f64 / 100.0,
        ),
        bar(
            &format!("Disk / · {:.1} / {:.1} GiB", disk.used_gib, disk.total_gib),
            disk.used_pct as f64 / 100.0,
        ),
        El::separator(),
        El::hbox(vec![
            El::label(format!("Battery: {battery}")).hexpand(true),
            El::label(format!("Net: {network}")).halign("end"),
        ]),
        El::button("refresh", "Refresh")
            .class("plugin-action plugin-action-primary")
            .hexpand(true),
    ])
    .spacing(12)
    .padding(12)
    .class("plugin-panel-body")
}

impl Component for SystemMonitor {
    fn view() -> El {
        view_tree()
    }
    fn update(_ev: Event) -> El {
        // Any click (only one button) re-evaluates view() and so re-reads /proc.
        if let EventKind::Click = _ev.kind {
            host::log(2, "system-monitor: refresh");
        }
        view_tree()
    }
}

export_component!(SystemMonitor);
