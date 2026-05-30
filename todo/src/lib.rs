//! Todo — a local, nestable task list for the margo shell, written with the
//! margo plugin authoring SDK (`mplugin-sdk`).
//!
//! Inspired by Dank Material Shell's `dms-dank-todo`: quick-add, one-click
//! toggle, inline edit, **subtasks** (unlimited nesting, indented), reorder
//! (▲/▼ — drag-and-drop isn't available in the WASM renderer), per-item delete
//! (cascades to subtasks), Clear completed, and **All / Active / Done** filter
//! chips. Items persist as JSON in the plugin's data dir; the bar pill shows a
//! live count via `count.sh`.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

const FILE: &str = "todos.json";
const DEFAULT_MAX_TODOS: usize = 200;
const DEFAULT_MAX_CHARS: usize = 500;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Active,
    Done,
}

#[derive(Clone)]
struct Node {
    id: u64,
    text: String,
    done: bool,
    children: Vec<Node>,
}

thread_local! {
    static ROOT: RefCell<Vec<Node>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
    static DRAFT: RefCell<String> = const { RefCell::new(String::new()) };
    static FILTER: RefCell<Filter> = const { RefCell::new(Filter::All) };
    /// Some(id) while editing that todo's text.
    static EDITING: RefCell<Option<u64>> = const { RefCell::new(None) };
    /// Some(id) while the next add becomes a subtask of that todo.
    static SUBTASK_OF: RefCell<Option<u64>> = const { RefCell::new(None) };
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

// ── Settings ──────────────────────────────────────────────────────────────

fn max_todos() -> usize {
    host::get_setting("max_todos").trim().parse().unwrap_or(DEFAULT_MAX_TODOS).max(1)
}

fn max_chars() -> usize {
    host::get_setting("max_chars").trim().parse().unwrap_or(DEFAULT_MAX_CHARS).max(1)
}

// ── Persistence (compact JSON, grep-friendly for count.sh) ──────────────────

fn node_from(v: &serde_json::Value) -> Option<Node> {
    let id = v["id"].as_u64()?;
    let text = v["text"].as_str()?.to_string();
    let done = v["done"].as_bool().unwrap_or(false);
    let children = v["children"]
        .as_array()
        .map(|a| a.iter().filter_map(node_from).collect())
        .unwrap_or_default();
    Some(Node { id, text, done, children })
}

fn node_to(n: &Node) -> serde_json::Value {
    serde_json::json!({
        "id": n.id,
        "text": n.text,
        "done": n.done,
        "children": n.children.iter().map(node_to).collect::<Vec<_>>(),
    })
}

fn ensure_loaded() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);

    let Ok(bytes) = host::read_file(FILE) else {
        return;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    if let Some(arr) = v["todos"].as_array() {
        let root: Vec<Node> = arr.iter().filter_map(node_from).collect();
        ROOT.with(|r| *r.borrow_mut() = root);
    }
    let next = v["next_id"]
        .as_u64()
        .unwrap_or_else(|| ROOT.with(|r| max_id(&r.borrow()) + 1));
    NEXT_ID.with(|n| *n.borrow_mut() = next.max(1));
}

fn max_id(nodes: &[Node]) -> u64 {
    nodes
        .iter()
        .map(|n| n.id.max(max_id(&n.children)))
        .max()
        .unwrap_or(0)
}

fn save() {
    let (todos, next) = (
        ROOT.with(|r| r.borrow().iter().map(node_to).collect::<Vec<_>>()),
        NEXT_ID.with(|n| *n.borrow()),
    );
    let v = serde_json::json!({ "version": 1, "next_id": next, "todos": todos });
    let _ = host::write_file(FILE, &v.to_string().into_bytes());
}

// ── Tree operations ─────────────────────────────────────────────────────────

fn count_nodes(nodes: &[Node]) -> (usize, usize) {
    let mut total = 0;
    let mut done = 0;
    for n in nodes {
        total += 1;
        if n.done {
            done += 1;
        }
        let (t, d) = count_nodes(&n.children);
        total += t;
        done += d;
    }
    (total, done)
}

fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = *n.borrow();
        *n.borrow_mut() = id + 1;
        id
    })
}

fn find_mut<'a>(nodes: &'a mut [Node], id: u64) -> Option<&'a mut Node> {
    for n in nodes.iter_mut() {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = find_mut(&mut n.children, id) {
            return Some(found);
        }
    }
    None
}

fn remove(nodes: &mut Vec<Node>, id: u64) -> bool {
    if let Some(pos) = nodes.iter().position(|n| n.id == id) {
        nodes.remove(pos);
        return true;
    }
    for n in nodes.iter_mut() {
        if remove(&mut n.children, id) {
            return true;
        }
    }
    false
}

/// Move `id` up/down among its siblings (the only reorder the renderer allows —
/// no drag-and-drop). Returns true once handled.
fn move_sibling(nodes: &mut Vec<Node>, id: u64, up: bool) -> bool {
    if let Some(pos) = nodes.iter().position(|n| n.id == id) {
        if up && pos > 0 {
            nodes.swap(pos, pos - 1);
        } else if !up && pos + 1 < nodes.len() {
            nodes.swap(pos, pos + 1);
        }
        return true;
    }
    for n in nodes.iter_mut() {
        if move_sibling(&mut n.children, id, up) {
            return true;
        }
    }
    false
}

fn add_todo(text: String, parent: Option<u64>) {
    let (total, _) = ROOT.with(|r| count_nodes(&r.borrow()));
    if total >= max_todos() {
        host::notify("Todo", "Reached the maximum number of todos.");
        return;
    }
    let text: String = text.chars().take(max_chars()).collect();
    let node = Node { id: next_id(), text, done: false, children: Vec::new() };
    ROOT.with(|r| match parent {
        Some(pid) => {
            let mut root = r.borrow_mut();
            if let Some(p) = find_mut(&mut root, pid) {
                p.children.push(node);
            } else {
                root.push(node);
            }
        }
        None => r.borrow_mut().push(node),
    });
    save();
}

// ── UI ──────────────────────────────────────────────────────────────────────

fn clear_modes() {
    EDITING.with(|e| *e.borrow_mut() = None);
    SUBTASK_OF.with(|s| *s.borrow_mut() = None);
    DRAFT.with(|d| d.borrow_mut().clear());
}

/// Find a todo's text by id (for the mode chips), if present.
fn text_of(nodes: &[Node], id: u64) -> Option<String> {
    for n in nodes {
        if n.id == id {
            return Some(n.text.clone());
        }
        if let Some(t) = text_of(&n.children, id) {
            return Some(t);
        }
    }
    None
}

fn input_area() -> El {
    let draft = DRAFT.with(|d| d.borrow().clone());
    let editing = EDITING.with(|e| *e.borrow());
    let subtask = SUBTASK_OF.with(|s| *s.borrow());

    let mut children: Vec<El> = Vec::new();

    // Mode chip (Editing / Subtask of …) with a cancel ✕.
    let mode_chip = if editing.is_some() {
        Some(("Editing".to_string(), ()))
    } else if let Some(pid) = subtask {
        let name = ROOT.with(|r| text_of(&r.borrow(), pid)).unwrap_or_default();
        Some((format!("Subtask of: {name}"), ()))
    } else {
        None
    };
    if let Some((label, ())) = mode_chip {
        children.push(
            El::hbox(vec![
                El::label(label).class("dim-label").halign("start").hexpand(true),
                El::button("cancel-mode", "")
                    .prop("icon", "edit-clear-symbolic")
                    .class("plugin-panel-action"),
            ])
            .spacing(8),
        );
    }

    let (add_id, add_icon) = if editing.is_some() {
        ("save-edit", "object-select-symbolic")
    } else {
        ("add", "list-add-symbolic")
    };
    children.push(
        El::hbox(vec![
            El::entry("draft", &draft).class("plugin-search").hexpand(true),
            El::button(add_id, "").prop("icon", add_icon).class("plugin-action plugin-action-primary"),
        ])
        .spacing(8),
    );
    El::vbox(children).spacing(8)
}

fn filter_chips() -> El {
    let cur = FILTER.with(|f| *f.borrow());
    let chip = |id: &str, label: &str, f: Filter| {
        let class = if cur == f {
            "plugin-chip plugin-chip-on"
        } else {
            "plugin-chip"
        };
        El::button(id, label).class(class)
    };
    El::hbox(vec![
        chip("filter:all", "All", Filter::All),
        chip("filter:active", "Active", Filter::Active),
        chip("filter:done", "Done", Filter::Done),
    ])
    .spacing(8)
    .halign("center")
}

/// One todo row. `depth` indents subtasks; `reorder` shows ▲/▼ + subtask
/// (only in the All view, where the tree order/nesting is meaningful).
fn row_view(n: &Node, depth: i32, reorder: bool) -> El {
    let mut row: Vec<El> = Vec::new();
    if depth > 0 {
        // Indent spacer for subtasks.
        row.push(El::label("").prop("width", (depth * 18).to_string()));
    }
    row.push(
        El::button(format!("toggle:{}", n.id), if n.done { "✓" } else { "○" })
            .class("plugin-panel-action"),
    );
    let label_class = if n.done { "dim-label" } else { "" };
    row.push(El::label(n.text.clone()).class(label_class).halign("start").hexpand(true));

    if reorder {
        row.push(
            El::button(format!("up:{}", n.id), "")
                .prop("icon", "go-up-symbolic")
                .class("plugin-panel-action"),
        );
        row.push(
            El::button(format!("down:{}", n.id), "")
                .prop("icon", "go-down-symbolic")
                .class("plugin-panel-action"),
        );
        row.push(
            El::button(format!("sub:{}", n.id), "")
                .prop("icon", "list-add-symbolic")
                .class("plugin-panel-action"),
        );
    }
    row.push(
        El::button(format!("edit:{}", n.id), "")
            .prop("icon", "document-edit-symbolic")
            .class("plugin-panel-action"),
    );
    row.push(
        El::button(format!("del:{}", n.id), "")
            .prop("icon", "user-trash-symbolic")
            .class("plugin-panel-action"),
    );

    El::hbox(row).spacing(4).padding(4).class("plugin-row")
}

/// Walk the tree in display order, emitting indented rows (All view).
fn push_tree(nodes: &[Node], depth: i32, out: &mut Vec<El>) {
    for n in nodes {
        out.push(row_view(n, depth, true));
        push_tree(&n.children, depth + 1, out);
    }
}

/// Flatten only the nodes matching the active filter (Active / Done view).
fn push_filtered(nodes: &[Node], want_done: bool, out: &mut Vec<El>) {
    for n in nodes {
        if n.done == want_done {
            out.push(row_view(n, 0, false));
        }
        push_filtered(&n.children, want_done, out);
    }
}

fn view_tree() -> El {
    ensure_loaded();
    let filter = FILTER.with(|f| *f.borrow());
    let (total, done) = ROOT.with(|r| count_nodes(&r.borrow()));
    let active = total - done;

    let header = El::hbox(vec![
        El::image("checkbox-checked-symbolic"),
        El::vbox(vec![
            El::label("Todo").class("label-large-bold").halign("start"),
            El::label(format!("{active} active · {total} total"))
                .class("dim-label")
                .halign("start"),
        ])
        .spacing(4)
        .hexpand(true),
    ])
    .class("plugin-panel-header")
    .spacing(12);

    // Build the list for the active filter.
    let mut rows: Vec<El> = Vec::new();
    ROOT.with(|r| {
        let root = r.borrow();
        match filter {
            Filter::All => push_tree(&root, 0, &mut rows),
            Filter::Active => push_filtered(&root, false, &mut rows),
            Filter::Done => push_filtered(&root, true, &mut rows),
        }
    });

    let list: El = if rows.is_empty() {
        let msg = match filter {
            Filter::All => "No todos yet — add one above.",
            Filter::Active => "Nothing active. Nice.",
            Filter::Done => "Nothing completed yet.",
        };
        El::label(msg).class("dim-label").halign("center").padding(16)
    } else {
        El::scroll(rows).class("plugin-list").vexpand(true).spacing(4)
    };

    let mut children = vec![header, input_area(), filter_chips(), El::separator(), list];

    if done > 0 {
        children.push(El::separator());
        children.push(
            El::button("clear-done", "Clear completed")
                .prop("icon", "user-trash-symbolic")
                .class("plugin-action plugin-action-danger"),
        );
    }

    El::vbox(children).spacing(12).class("plugin-panel-large")
}

// ── Component impl ───────────────────────────────────────────────────────────

struct Todo;

/// The text to commit on add/save: the draft, trimmed.
fn draft_text() -> String {
    DRAFT.with(|d| d.borrow().trim().to_string())
}

impl Component for Todo {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Submit if ev.id == "draft" => {
                DRAFT.with(|d| *d.borrow_mut() = ev.value.clone());
                commit_input();
            }
            EventKind::Click => {
                let id = ev.id.as_str();
                if let Some(rest) = id.strip_prefix("toggle:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        ROOT.with(|r| {
                            if let Some(n) = find_mut(&mut r.borrow_mut(), tid) {
                                n.done = !n.done;
                            }
                        });
                        save();
                    }
                } else if let Some(rest) = id.strip_prefix("del:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        ROOT.with(|r| {
                            remove(&mut r.borrow_mut(), tid);
                        });
                        // If we were editing/subtasking that node, drop the mode.
                        if EDITING.with(|e| *e.borrow()) == Some(tid)
                            || SUBTASK_OF.with(|s| *s.borrow()) == Some(tid)
                        {
                            clear_modes();
                        }
                        save();
                    }
                } else if let Some(rest) = id.strip_prefix("up:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        ROOT.with(|r| {
                            move_sibling(&mut r.borrow_mut(), tid, true);
                        });
                        save();
                    }
                } else if let Some(rest) = id.strip_prefix("down:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        ROOT.with(|r| {
                            move_sibling(&mut r.borrow_mut(), tid, false);
                        });
                        save();
                    }
                } else if let Some(rest) = id.strip_prefix("edit:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        let text = ROOT.with(|r| text_of(&r.borrow(), tid));
                        if let Some(text) = text {
                            SUBTASK_OF.with(|s| *s.borrow_mut() = None);
                            EDITING.with(|e| *e.borrow_mut() = Some(tid));
                            DRAFT.with(|d| *d.borrow_mut() = text);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("sub:") {
                    if let Ok(tid) = rest.parse::<u64>() {
                        EDITING.with(|e| *e.borrow_mut() = None);
                        SUBTASK_OF.with(|s| *s.borrow_mut() = Some(tid));
                        DRAFT.with(|d| d.borrow_mut().clear());
                    }
                } else if let Some(rest) = id.strip_prefix("filter:") {
                    let f = match rest {
                        "active" => Filter::Active,
                        "done" => Filter::Done,
                        _ => Filter::All,
                    };
                    FILTER.with(|x| *x.borrow_mut() = f);
                } else {
                    match id {
                        "add" | "save-edit" => commit_input(),
                        "cancel-mode" => clear_modes(),
                        "clear-done" => {
                            ROOT.with(|r| clear_done(&mut r.borrow_mut()));
                            save();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        view_tree()
    }
}

/// Remove every completed node (cascading — a done parent takes its subtree).
fn clear_done(nodes: &mut Vec<Node>) {
    nodes.retain(|n| !n.done);
    for n in nodes.iter_mut() {
        clear_done(&mut n.children);
    }
}

/// Apply the input: save an edit, add a subtask, or add a top-level todo,
/// depending on the active mode. Clears the draft + mode afterwards.
fn commit_input() {
    let text = draft_text();
    if text.is_empty() {
        return;
    }
    if let Some(tid) = EDITING.with(|e| *e.borrow()) {
        let capped: String = text.chars().take(max_chars()).collect();
        ROOT.with(|r| {
            if let Some(n) = find_mut(&mut r.borrow_mut(), tid) {
                n.text = capped;
            }
        });
        save();
    } else if let Some(pid) = SUBTASK_OF.with(|s| *s.borrow()) {
        add_todo(text, Some(pid));
    } else {
        add_todo(text, None);
    }
    clear_modes();
}

export_component!(Todo);
