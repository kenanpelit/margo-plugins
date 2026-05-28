# margo-plugins

Community widget plugins for the [margo](https://github.com/kenanpelit/margo)
desktop shell (**mshell**) — add this repo as a *source* in
**Settings → Widgets → Plugins** and install widgets straight from the shell.

> **Status:** the mshell plugin manager is in development. This repository
> defines the source/registry layout that the manager consumes; the format
> below may still evolve before the first release.

## How it works

A *source* is any git repository with a `registry.toml` at its root. mshell
shallow-clones that file to list the available plugins, then sparse-clones an
individual plugin's folder when you install it. Installed plugins are
declarative — they don't ship compiled code; each one describes one or more
bar widgets (icon, label, a command to poll for the label, click actions)
that mshell renders with its built-in custom-widget engine.

To use this source:

1. Open **Settings → Widgets → Plugins → Sources** and add this repo's URL.
2. Browse **Available**, install a plugin, then enable it.
3. Place its widget into a bar slot like any other widget.

## Repository layout

```
margo-plugins/
├── registry.toml          # index of every plugin in this repo
├── <plugin-id>/
│   ├── manifest.toml       # plugin metadata + widget definitions
│   └── assets/             # optional icons/images referenced by the manifest
└── …
```

### `registry.toml`

The root index. One `[[plugins]]` entry per plugin:

```toml
[[plugins]]
id = "weather-tr"          # unique within this source
dir = "weather-tr"          # folder holding the plugin's manifest.toml
version = "1.2.0"           # must match the manifest
name = "Türkiye Weather"
min_mshell = "0.8.8"        # minimum mshell version required
```

### `<plugin-id>/manifest.toml`

```toml
id = "weather-tr"
name = "Türkiye Weather"
version = "1.2.0"
author = "your-handle"
min_mshell = "0.8.8"
description = "wttr.in-backed weather pill"

# A plugin may declare one or more widgets.
[[widget]]
key = "current"                          # placed in a bar as plugin:weather-tr:current
icon = "weather-few-clouds-symbolic"     # symbolic icon name (optional)
exec = "curl -s 'wttr.in/Istanbul?format=%t'"   # stdout becomes the label
template = "{output}"                    # {output} = trimmed exec stdout
interval = 900                           # refresh seconds (0 = run once)
on_click = "xdg-open https://wttr.in/Istanbul"   # optional left-click command
```

## Contributing a plugin

1. Add a `<plugin-id>/` folder with a `manifest.toml` (and any `assets/`).
2. Add a matching `[[plugins]]` entry to `registry.toml`.
3. Open a pull request.

**Security note:** plugins run shell commands (`exec` / `on_click`) with your
user privileges — install only plugins you trust, and review the commands a
plugin declares before enabling it.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE). Individual plugins may carry their
own license inside their folder.
