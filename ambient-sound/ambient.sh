#!/usr/bin/env sh
# Ambient sound playback helper for the margo ambient-sound plugin.
#
# Invoked by the WASM panel via host::run("sh", [this, DIR, CMD, args...]).
# It backgrounds one detached mpv per sound, each with its own JSON IPC
# socket so the master volume can be pushed live. Every call returns fast
# (mpv is detached with setsid + `&`), so the host's blocking `run` never
# stalls the UI.
#
# Needs: mpv, socat, setsid (util-linux), pkill/pgrep (procps).

DIR="$1"
CMD="$2"
shift 2 2>/dev/null

PFX="/tmp/margo-ambient-"

sock_for() { printf '%s%s.sock' "$PFX" "$1"; }

case "$CMD" in
  play)
    sound="$1"
    vol="${2:-75}"
    sock=$(sock_for "$sound")
    # Already running for this sound? leave it alone.
    if pgrep -f "input-ipc-server=$sock" >/dev/null 2>&1; then
      exit 0
    fi
    setsid mpv --no-video --no-config --no-terminal --loop=inf \
        --volume="$vol" --input-ipc-server="$sock" \
        "$DIR/sounds/$sound.ogg" >/dev/null 2>&1 </dev/null &
    ;;

  stop)
    sound="$1"
    sock=$(sock_for "$sound")
    pkill -f "input-ipc-server=$sock" 2>/dev/null
    rm -f "$sock"
    ;;

  stop-all)
    pkill -f "input-ipc-server=$PFX" 2>/dev/null
    rm -f "$PFX"*.sock 2>/dev/null
    ;;

  vol-all)
    # Push one volume (0-100) to every live socket — used for master volume
    # changes and mute/unmute.
    vol="$1"
    for sock in "$PFX"*.sock; do
      [ -S "$sock" ] || continue
      printf '{"command":["set_property","volume",%s]}\n' "$vol" \
        | socat - "UNIX-CONNECT:$sock" >/dev/null 2>&1
    done
    ;;

  status)
    # Echo, one per line, which of the named sounds are currently playing —
    # lets the panel reconcile its state with reality after a shell restart.
    for s in "$@"; do
      sock=$(sock_for "$s")
      if pgrep -f "input-ipc-server=$sock" >/dev/null 2>&1; then
        printf '%s\n' "$s"
      fi
    done
    ;;

  timer)
    # Detached sleep timer: after <minutes>, run <action> (stopall|lock|
    # suspend). Tagged "margoambienttimer" in its argv so `timer-cancel`
    # can find and kill it.
    minutes="$1"
    action="${2:-stopall}"
    pkill -f "margoambienttimer" 2>/dev/null
    if ! [ "${minutes:-0}" -gt 0 ] 2>/dev/null; then
      exit 0
    fi
    secs=$((minutes * 60))
    setsid sh -c '
      sleep '"$secs"'
      case "'"$action"'" in
        lock)
          pkill -f "input-ipc-server='"$PFX"'"; rm -f '"$PFX"'*.sock
          loginctl lock-session
          ;;
        suspend)
          pkill -f "input-ipc-server='"$PFX"'"; rm -f '"$PFX"'*.sock
          systemctl suspend
          ;;
        *)
          pkill -f "input-ipc-server='"$PFX"'"; rm -f '"$PFX"'*.sock
          ;;
      esac
    ' margoambienttimer >/dev/null 2>&1 </dev/null &
    ;;

  timer-cancel)
    pkill -f "margoambienttimer" 2>/dev/null
    ;;
esac

exit 0
