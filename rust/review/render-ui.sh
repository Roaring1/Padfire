#!/usr/bin/env bash
set -euo pipefail

binary="${1:?usage: render-ui.sh /path/to/padfire /path/to/output-dir}"
output_dir="${2:?usage: render-ui.sh /path/to/padfire /path/to/output-dir}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -x "$binary" ]]; then
  echo "Rust app binary is missing or not executable: $binary" >&2
  exit 1
fi
for command in xdotool import identify; do
  command -v "$command" >/dev/null || {
    echo "Required UI review tool not found: $command" >&2
    exit 1
  }
done

work_dir="$(mktemp -d)"
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    for _ in {1..30}; do
      kill -0 "$app_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  rm -rf "$work_dir"
}
trap cleanup EXIT

home="$work_dir/home"
runtime_dir="$work_dir/runtime"
mkdir -p "$home/.config/padfire" "$home/.local/share" "$runtime_dir" "$output_dir"
chmod 700 "$runtime_dir"
cp "$script_dir/padfire-ui-review.json" "$home/.config/padfire/padfire.json"

export HOME="$home"
export XDG_CONFIG_HOME="$home/.config"
export XDG_DATA_HOME="$home/.local/share"
export XDG_RUNTIME_DIR="$runtime_dir"
export PADFIRE_DISABLE_HARDWARE=1
export LIBGL_ALWAYS_SOFTWARE=1
export RUST_LOG=warn
# Keep the virtual desktop isolated from the user's live KDE session, tray,
# audio server and Wayland socket.
unset DBUS_SESSION_BUS_ADDRESS WAYLAND_DISPLAY SESSION_MANAGER || true
# Preserve XAUTHORITY from xvfb-run; it owns the temporary X server cookie.
# Do not inherit a desktop bus or Wayland socket from the caller.
export PULSE_SERVER="unix:$runtime_dir/no-pulse-server"

"$binary" >"$output_dir/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in {1..100}; do
  window_id="$(xdotool search --onlyvisible --name 'Padfire' 2>/dev/null | head -n 1 || true)"
  if [[ -n "$window_id" ]]; then
    break
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "Padfire exited before its window appeared" >&2
    cat "$output_dir/app.log" >&2 || true
    exit 1
  fi
  sleep 0.1
done

if [[ -z "$window_id" ]]; then
  echo "Timed out waiting for the Padfire window" >&2
  cat "$output_dir/app.log" >&2 || true
  exit 1
fi

xdotool windowactivate --sync "$window_id" 2>/dev/null || true
sleep 0.8
import -window "$window_id" "$output_dir/01-main-grid.png"

window_width="$(xdotool getwindowgeometry "$window_id" | awk '/Geometry:/ { sub(/x.*/, "", $2); print $2 }')"
[[ "$window_width" =~ ^[0-9]+$ ]] || {
  echo "Could not determine Padfire window width" >&2
  exit 1
}
xdotool mousemove --window "$window_id" "$((window_width - 38))" 20 click 1
sleep 0.5
import -window "$window_id" "$output_dir/02-settings.png"

for image in "$output_dir/01-main-grid.png" "$output_dir/02-settings.png"; do
  [[ -s "$image" ]] || { echo "Screenshot is missing: $image" >&2; exit 1; }
  dimensions="$(identify -format '%wx%h' "$image")"
  [[ "$dimensions" =~ ^[0-9]+x[0-9]+$ ]] || {
    echo "Invalid screenshot dimensions: $image ($dimensions)" >&2
    exit 1
  }
  width="${dimensions%x*}"
  height="${dimensions#*x}"
  (( width >= 480 && height >= 480 )) || {
    echo "Screenshot is unexpectedly small: $image ($dimensions)" >&2
    exit 1
  }
  printf 'PASS screenshot %s (%s)\n' "$(basename "$image")" "$dimensions"
done

if cmp -s "$output_dir/01-main-grid.png" "$output_dir/02-settings.png"; then
  echo "Settings click did not change the rendered UI; refusing to publish duplicate screenshots" >&2
  exit 1
fi

"$binary" --version >"$output_dir/version.txt"
