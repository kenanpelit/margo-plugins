#!/usr/bin/env sh
# Bar-pill count for the margo todo plugin. Reads the plugin's JSON store and
# prints the count for the requested mode (active | total | done | hidden).
# Invoked by the manifest:  sh {{plugin_dir}}/count.sh {{count_mode}}
#
# No jq dependency — the store is compact JSON (serde_json), so each todo node
# carries a `"id":` and a `"done":true|false`, which grep can tally.

mode="${1:-active}"
[ "$mode" = "hidden" ] && exit 0

f="${XDG_DATA_HOME:-$HOME/.local/share}/mshell/plugins/todo/todos.json"
[ -f "$f" ] || exit 0

total=$(grep -o '"id":' "$f" | wc -l | tr -d ' ')
done=$(grep -o '"done":true' "$f" | wc -l | tr -d ' ')
active=$((total - done))

case "$mode" in
  total) [ "$total"  -gt 0 ] && printf '%s' "$total" ;;
  done)  [ "$done"   -gt 0 ] && printf '%s' "$done" ;;
  *)     [ "$active" -gt 0 ] && printf '%s' "$active" ;;
esac
exit 0
