# margo plugin template

Scaffold a new margo WASM plugin in one command:

```sh
cargo install cargo-generate
cargo generate --git https://github.com/kenanpelit/margo-plugins \
               --branch main \
               --name my-plugin \
               template
```

You get:

- `Cargo.toml` already wired to the `mplugin-sdk` (`git`, branch `main`).
- `manifest.toml` with a working `[[widget]]` opening the WASM panel.
- `src/lib.rs` with a minimal counter plugin you can iterate on.

## Development loop

```sh
cargo build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/my_plugin.wasm \
   ~/.config/margo/mshell/plugins/my-plugin/plugin.wasm
```

mshell's file watcher picks the new `plugin.wasm` up automatically — the next
time you open the panel you see the change. No restart needed.

Add the plugin to the registry index (`registry.toml` in this repo) when
you're ready to publish it so other users can install it from Settings →
Plugins.
