//! {{display_name}} — a margo plugin scaffolded from the official template.
//!
//! Run during development:
//!   cargo build --target wasm32-wasip2 --release \
//!     && cp target/wasm32-wasip2/release/{{crate_name}}.wasm \
//!           ~/.config/margo/mshell/plugins/{{plugin_id}}/plugin.wasm
//! mshell's file watcher hot-reloads the panel automatically.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

thread_local! {
    static COUNTER: RefCell<u32> = const { RefCell::new(0) };
}

struct Plugin;

fn view_tree() -> El {
    let count = COUNTER.with(|c| *c.borrow());
    El::vbox(vec![
        El::markdown(format!(
            "**{{display_name}}** — clicked **{count}** times"
        ))
        .class("plugin-hero"),
        El::button("bump", "Click me").class("plugin-action plugin-action-primary"),
    ])
    .spacing(12)
    .padding(12)
    .class("plugin-panel-body")
}

impl Component for Plugin {
    fn view() -> El {
        host::log(2, "{{plugin_id}}: view");
        view_tree()
    }

    fn update(ev: Event) -> El {
        if let EventKind::Click = ev.kind {
            if ev.id == "bump" {
                COUNTER.with(|c| {
                    let mut c = c.borrow_mut();
                    *c += 1;
                });
            }
        }
        view_tree()
    }
}

export_component!(Plugin);
