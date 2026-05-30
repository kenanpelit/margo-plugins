# todo (margo)

A local, nestable task list for the margo shell. Quick-add, one-click toggle,
inline edit, **subtasks**, reordering, and **All / Active / Done** filters —
all in an in-shell panel. Inspired by Dank Material Shell's `dms-dank-todo`,
ported to margo's WASM plugin tier (`mplugin-sdk`).

## Features

- **Quick-add** — type in the box, Enter (or the + button) to add.
- **Toggle** — click the ○/✓ to mark a todo active/done.
- **Inline edit** — ✎ loads the row's text into the box ("Editing…"); Enter or
  ✓ saves, the × on the chip cancels.
- **Subtasks** — ＋ on a row switches the box to "Subtask of: …"; the next add
  becomes a child. Unlimited nesting, indented in the list.
- **Reorder** — ▲ / ▼ move a todo among its siblings (the WASM renderer has no
  drag-and-drop, so reordering is by buttons).
- **Delete** — 🗑 removes a todo and its subtasks.
- **Filters** — All (nested tree) / Active / Done (flat).
- **Clear completed** — drops every done todo (and its subtree).
- **Live bar pill** — the badge shows the active / total / done count (or is
  hidden), per the **Bar pill count** setting.
- **Limits** — Maximum todos (default 200) and Max characters per todo
  (default 500).

## Storage

Todos persist as compact JSON in the plugin's data dir
(`$XDG_DATA_HOME/mshell/plugins/todo/todos.json`) via the host's scoped
write-file capability — no network, no shell-outs from the panel. Format:

```json
{
  "version": 1,
  "next_id": 5,
  "todos": [
    { "id": 1, "text": "Shopping", "done": false,
      "children": [ { "id": 2, "text": "Milk", "done": true, "children": [] } ] }
  ]
}
```

Array order is display order; `children` nests subtasks. The bar pill's count is
read from this file by `count.sh` (grep-based, no `jq` needed).

## Setup

1. Settings → Plugins → install/enable **Todo**. Optionally set the pill count
   mode and limits.
2. Place **Todo · main** in a bar (or use **Super+Shift+T**).
3. Click the pill to open the panel.

## Rebuilding `plugin.wasm`

```sh
cd todo
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/todo.wasm ./plugin.wasm
```

The `mplugin-sdk` path in `Cargo.toml` assumes the margo repo is a sibling of
this one (`~/.kod/margo` next to `~/.kod/margo-plugins`).
