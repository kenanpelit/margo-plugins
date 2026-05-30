#!/usr/bin/env sh
# Bar-pill "now playing" for the margo mplayerplus plugin (art = true).
# Invoked by the manifest:  sh {{plugin_dir}}/nowplaying.sh
#
# Output contract for an `art` widget:
#   line 1 → album-art file path (or empty)
#   line 2 → label text ("title — artist")
#
# Kept in a script (not the manifest exec) so playerctl's own {{…}} format
# tokens aren't eaten by the manifest's {{key}} substitution. Prefers a player
# that's actually playing; prints nothing when idle.

# Pick the first playing player, else playerctl's default.
player=$(playerctl -l 2>/dev/null | while read -r p; do
    [ "$(playerctl -p "$p" status 2>/dev/null)" = "Playing" ] && { printf '%s' "$p"; break; }
done)
[ -n "$player" ] && set -- -p "$player" || set --

art=$(playerctl "$@" metadata mpris:artUrl 2>/dev/null)
label=$(playerctl "$@" metadata --format '{{title}} — {{artist}}' 2>/dev/null)

# Only local files can be shown as a leading image; strip file:// (and ignore
# remote http art, which the bar can't load).
case "$art" in
    file://*) printf '%s\n' "${art#file://}" ;;
    /*)       printf '%s\n' "$art" ;;
    *)        printf '\n' ;;
esac
printf '%s\n' "$label"
