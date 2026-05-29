//! Todo — a tiny task list backed by the scoped fs capability. Items live
//! in `items.txt` in the plugin's data dir, one per line, formatted as
//! `<done>|<text>` (`1|Buy milk`, `0|Finish report`).

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    static ITEMS: RefCell<Vec<Item>> = const { RefCell::new(Vec::new()) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
    static DRAFT: RefCell<String> = const { RefCell::new(String::new()) };
}

#[derive(Clone)]
struct Item {
    text: String,
    done: bool,
}

const FILE: &str = "items.txt";

fn ensure_loaded() {
    LOADED.with(|l| {
        if *l.borrow() {
            return;
        }
        if let Ok(bytes) = host::read_file(FILE) {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                let items: Vec<Item> = text
                    .lines()
                    .filter_map(|line| {
                        let (done, t) = line.split_once('|')?;
                        if t.is_empty() {
                            return None;
                        }
                        Some(Item {
                            text: t.to_string(),
                            done: done == "1",
                        })
                    })
                    .collect();
                ITEMS.with(|i| *i.borrow_mut() = items);
            }
        }
        *l.borrow_mut() = true;
    });
}

fn save() {
    let blob = ITEMS.with(|i| {
        i.borrow()
            .iter()
            .map(|it| format!("{}|{}", if it.done { 1 } else { 0 }, it.text))
            .collect::<Vec<_>>()
            .join("\n")
    });
    let _ = host::write_file(FILE, blob.as_bytes());
}

fn row_view(index: usize, item: &Item) -> El {
    let (label, label_class) = if item.done {
        (format!("✓ {}", item.text), "dim-label")
    } else {
        (item.text.clone(), "")
    };
    El::hbox(vec![
        El::button(format!("toggle:{index}"), if item.done { "✓" } else { "·" })
            .class("plugin-toggle"),
        El::label(label).class(label_class).hexpand(true).halign("start"),
        El::button(format!("delete:{index}"), "✕").class("plugin-toggle"),
    ])
    .spacing(8)
    .padding(4)
    .class("plugin-row")
}

fn view_tree() -> El {
    ensure_loaded();
    let items = ITEMS.with(|i| i.borrow().clone());
    let draft = DRAFT.with(|d| d.borrow().clone());
    let pending = items.iter().filter(|i| !i.done).count();

    let mut children = vec![
        El::markdown(format!(
            "**Todo** — {} pending · {} total",
            pending,
            items.len()
        ))
        .class("plugin-hero"),
        El::hbox(vec![
            El::entry("draft", &draft)
                .class("plugin-search")
                .hexpand(true),
            El::button("add", "Add").class("plugin-action plugin-action-primary"),
        ])
        .spacing(6),
        El::separator(),
    ];

    if items.is_empty() {
        children.push(El::label("No todos yet — add one above.").class("dim-label"));
    } else {
        let rows: Vec<El> = items
            .iter()
            .enumerate()
            .map(|(i, it)| row_view(i, it))
            .collect();
        children.push(El::scroll(rows).class("plugin-list").vexpand(true));
    }

    if items.iter().any(|i| i.done) {
        children.push(El::separator());
        children.push(
            El::button("clear-done", "Clear completed")
                .class("plugin-action plugin-action-danger"),
        );
    }

    El::vbox(children)
        .spacing(10)
        .padding(12)
        .class("plugin-panel-body")
}

struct Todo;

impl Component for Todo {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Submit if ev.id == "draft" => {
                let text = ev.value.trim().to_string();
                if !text.is_empty() {
                    ITEMS.with(|i| {
                        i.borrow_mut().push(Item { text, done: false });
                    });
                    DRAFT.with(|d| d.borrow_mut().clear());
                    save();
                }
            }
            EventKind::Click => match ev.id.as_str() {
                "add" => {
                    let text = DRAFT.with(|d| d.borrow().trim().to_string());
                    if !text.is_empty() {
                        ITEMS.with(|i| {
                            i.borrow_mut().push(Item { text, done: false });
                        });
                        DRAFT.with(|d| d.borrow_mut().clear());
                        save();
                    }
                }
                "clear-done" => {
                    ITEMS.with(|i| i.borrow_mut().retain(|it| !it.done));
                    save();
                }
                id if id.starts_with("toggle:") => {
                    if let Some(n) = id[7..].parse::<usize>().ok() {
                        ITEMS.with(|i| {
                            if let Some(it) = i.borrow_mut().get_mut(n) {
                                it.done = !it.done;
                            }
                        });
                        save();
                    }
                }
                id if id.starts_with("delete:") => {
                    if let Some(n) = id[7..].parse::<usize>().ok() {
                        ITEMS.with(|i| {
                            let mut v = i.borrow_mut();
                            if n < v.len() {
                                v.remove(n);
                            }
                        });
                        save();
                    }
                }
                _ => {}
            },
            _ => {}
        }
        view_tree()
    }
}

export_component!(Todo);
