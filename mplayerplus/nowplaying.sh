#!/usr/bin/env sh
# Bar-pill "now playing" for the margo mplayerplus plugin (art = true).
# Invoked by the manifest:  sh {{plugin_dir}}/nowplaying.sh
#
# Output contract for an `art` widget (up to three lines):
#   line 1 → album-art file path (or empty)
#   line 2 → label text ("title — artist")
#   line 3 → status ("playing" | "paused" | "stopped") — dims the pill
#
# Kept in a script (not the manifest exec) so playerctl's own {{…}} format
# tokens aren't eaten by the manifest's {{key}} substitution.

# Pick the first playing player, else playerctl's default.
player=$(playerctl -l 2>/dev/null | while read -r p; do
    [ "$(playerctl -p "$p" status 2>/dev/null)" = "Playing" ] && { printf '%s' "$p"; break; }
done)
[ -n "$player" ] && set -- -p "$player" || set --

art=$(playerctl "$@" metadata mpris:artUrl 2>/dev/null)
label=$(playerctl "$@" metadata --format '{{title}} — {{artist}}' 2>/dev/null)
status=$(playerctl "$@" status 2>/dev/null | tr '[:upper:]' '[:lower:]')

# Resolve the artwork to a local path. Spotify (and most streaming players)
# hand out a remote https URL — download + cache it so the bar can show it,
# the way the built-in media pill does via wayle's art cache. YouTube/Chromium
# use a local file:// path already.
case "$art" in
    file://*)
        printf '%s\n' "${art#file://}"
        ;;
    http://*|https://*)
        cache="${XDG_CACHE_HOME:-$HOME/.cache}/margo-mplayerplus"
        mkdir -p "$cache" 2>/dev/null
        key=$(printf '%s' "$art" | md5sum | cut -d' ' -f1)
        f="$cache/$key"
        [ -s "$f" ] || curl -sL --max-time 8 -o "$f" "$art" 2>/dev/null
        [ -s "$f" ] && printf '%s\n' "$f" || printf '\n'
        ;;
    /*)
        printf '%s\n' "$art"
        ;;
    *)
        printf '\n'
        ;;
esac
printf '%s\n' "$label"
printf '%s\n' "$status"
