#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)"
ROOT="$(cd -- "$HERE/../.." && pwd)"
PADFIRE_PY="${PADFIRE_PY:-$ROOT/padfire.py}"
PADFIRE_LAUNCH="${PADFIRE_LAUNCH:-$ROOT/padfire-launch}"
LOG="${PADFIRE_LOG:-$HOME/.cache/padfire.log}"

[[ -f "$PADFIRE_PY" ]] || { echo "error: Python source not found: $PADFIRE_PY" >&2; exit 1; }
[[ -x "$PADFIRE_LAUNCH" ]] || { echo "error: launcher is not executable: $PADFIRE_LAUNCH" >&2; exit 1; }

python3 -B -m py_compile "$PADFIRE_PY"
echo "Python syntax OK"

is_running() {
    local status
    if ! status="$("$PADFIRE_LAUNCH" --status 2>/dev/null)"; then
        return 2
    fi
    python3 -c '
import json, sys
try:
    state = json.loads(sys.argv[1])
except (json.JSONDecodeError, IndexError):
    raise SystemExit(2)
if not isinstance(state, dict):
    raise SystemExit(2)
raise SystemExit(0 if state.get("running", True) else 1)
' "$status"
}

# Ask the owned instance to quit through its own IPC. Never pkill broad
# process-name patterns and never unlink its socket: another instance may own it.
if is_running; then
    echo "Requesting graceful Padfire shutdown..."
    if ! "$PADFIRE_LAUNCH" --quit; then
        echo "error: Padfire did not acknowledge --quit; refusing to force-kill it." >&2
        exit 1
    fi

    stopped=0
    for _ in {1..40}; do
        if is_running; then
            sleep 0.25
            continue
        else
            status_result=$?
            if (( status_result == 1 )); then
                stopped=1
                break
            fi
            echo "error: Could not verify Padfire shutdown; refusing to continue." >&2
            exit 1
        fi
    done
    if (( ! stopped )); then
        echo "error: Padfire is still responding after 10 seconds; refusing to remove its socket or kill it." >&2
        exit 1
    fi
else
    status_result=$?
    if (( status_result != 1 )); then
        echo "error: Could not verify Padfire status; refusing to start a duplicate instance." >&2
        exit 1
    fi
fi

umask 077
mkdir -p "$(dirname "$LOG")"
touch "$LOG"
chmod 600 "$LOG"

echo "Starting Padfire..."
PADFIRE_PY="$PADFIRE_PY" nohup "$PADFIRE_LAUNCH" >>"$LOG" 2>&1 </dev/null &
launch_pid=$!
disown "$launch_pid" 2>/dev/null || true

started=0
for _ in {1..40}; do
    if is_running; then
        started=1
        break
    fi
    if ! kill -0 "$launch_pid" 2>/dev/null; then
        break
    fi
    sleep 0.25
done

if (( ! started )); then
    echo "error: Padfire did not publish a live IPC status; inspect $LOG" >&2
    exit 1
fi

echo "Padfire is running."
tail -n 8 "$LOG"
