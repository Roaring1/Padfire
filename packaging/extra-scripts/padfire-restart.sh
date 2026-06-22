#!/bin/bash
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PADFIRE_PY="${PADFIRE_PY:-$ROOT/padfire.py}"
PADFIRE_LAUNCH="${PADFIRE_LAUNCH:-$ROOT/padfire-launch}"
LOG="${PADFIRE_LOG:-$HOME/.cache/padfire.log}"
SOCK="${PADFIRE_SOCK:-$HOME/.config/padfire/padfire.sock}"

python3 -B -m py_compile "$PADFIRE_PY" && echo "SYNTAX OK"
pkill -f "padfire.py|padfire-launch" 2>/dev/null || true
sleep 1
mkdir -p "$(dirname "$LOG")"
rm -f "$SOCK" "$LOG"
PADFIRE_PY="$PADFIRE_PY" nohup "$PADFIRE_LAUNCH" >> "$LOG" 2>&1 &
disown $!
echo "launched"
sleep 5
tail -4 "$LOG"
