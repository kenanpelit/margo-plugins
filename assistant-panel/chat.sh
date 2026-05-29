#!/usr/bin/env sh
# Minimal multi-turn Gemini chat for the margo assistant-panel plugin.
# Run by the plugin via:  GEMINI_API_KEY=… sh chat.sh [model]
# Needs: curl, jq.

model="${1:-gemini-2.5-flash}"
case "$model" in
    ""|flash|gemini-flash) model="gemini-2.5-flash" ;;
    pro|gemini-pro) model="gemini-2.5-pro" ;;
esac

if [ -z "$GEMINI_API_KEY" ]; then
    echo "No API key set — open Settings → Plugins → Assistant Panel and set one."
    printf 'Press Enter to close… '
    read -r _
    exit 1
fi

printf '\033[1mmargo assistant\033[0m · %s   (Ctrl-C / empty line + Ctrl-D to quit)\n' "$model"
hist='[]'

while :; do
    printf '\n\033[1;36myou>\033[0m '
    IFS= read -r q || break
    [ -z "$q" ] && continue
    hist=$(printf '%s' "$hist" | jq -c --arg q "$q" '. + [{role:"user",parts:[{text:$q}]}]')
    resp=$(curl -s \
        "https://generativelanguage.googleapis.com/v1beta/models/$model:generateContent" \
        -H 'Content-Type: application/json' \
        -H "x-goog-api-key: $GEMINI_API_KEY" \
        -d "$(printf '%s' "$hist" | jq -c '{contents: .}')")
    ans=$(printf '%s' "$resp" | jq -r '.candidates[0].content.parts[0].text // .error.message // "(no response)"')
    printf '\033[1;32mai>\033[0m %s\n' "$ans"
    hist=$(printf '%s' "$hist" | jq -c --arg a "$ans" '. + [{role:"model",parts:[{text:$a}]}]')
done
