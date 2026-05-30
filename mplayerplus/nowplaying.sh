#!/usr/bin/env sh
# Bar-pill "now playing" line for the margo mplayerplus plugin.
# Invoked by the manifest:  sh {{plugin_dir}}/nowplaying.sh
#
# Lives in a script (not the manifest's exec) so playerctl's own {{…}} format
# tokens aren't eaten by the manifest's {{key}} substitution.
#
# Prefers a player that's actually playing; falls back to whatever playerctl
# picks. Prints nothing (→ pill shows just its icon) when nothing is up.

# Find the first playing player, else the default.
player=$(playerctl -l 2>/dev/null | while read -r p; do
    [ "$(playerctl -p "$p" status 2>/dev/null)" = "Playing" ] && { printf '%s' "$p"; break; }
done)

if [ -n "$player" ]; then
    playerctl -p "$player" metadata --format '{{title}} — {{artist}}' 2>/dev/null
else
    playerctl metadata --format '{{title}} — {{artist}}' 2>/dev/null
fi
