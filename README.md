# Padfire

GTK3 soundboard for the Novation Launchpad Mini (MK1/MK2).

8 pages of 8x8 pads. Each pad plays a sound file via PipeWire (`paplay`). LED feedback with per-page colors. IPC socket so Hearth can show playback status.

---

## Features

- 8 pages (top-row CC 104-111), 64 assignable pads per page
- Per-pad: sound file, label, volume (0-150%), output sink, loop toggle, color
- Per-pad actions: `play`, `script`, `stop_sfx`, `stop_all`, `stop_key`
- LED feedback: pad lights on press, color reflects assignment state
- Missing sound files are shown on the pad itself: a dashed red border and a `!` glyph, so a broken assignment is visible before you press it
- Tray icon with a live page indicator (`P1`-`P8`) and stop-all
- IPC: `status`, `stop_all`, `show`, `reload`, `quit` over a Unix socket (`~/.config/padfire/padfire.sock`)
- Hearth integration: Padfire status panel in Hearth shows current page, playing sounds, MIDI connection state. When Hearth is not running, its controls hide instead of showing empty labels
- VU meter follows your configured monitor/output sink rather than a fixed one
- MIDI input via `mido` (rtmidi backend), auto-reconnect on device unplug/replug
- Audio via `paplay` subprocess -> PipeWire, no JACK/PortAudio dependency
- Loudness analysis (`--` LUFS target -18.0) available from the pad editor

---

## Requirements

- Python 3.10+
- GTK3 (`python-gobject`)
- `mido` + `python-rtmidi`
- `paplay` (PipeWire / pipewire-pulse)
- `parec` (optional -- VU meter; the meter hides itself if absent)
- `libappindicator3` (tray, optional)
- Novation Launchpad Mini MK1 or MK2

```bash
# Fedora / Nobara / RHEL-like
sudo dnf install python3-gobject python3-mido python3-rtmidi libappindicator-gtk3

# Arch
sudo pacman -S python-gobject python-mido python-rtmidi libappindicator-gtk3

# Any distro, if the packaged MIDI bindings are missing or too old
pip install --user mido python-rtmidi
```

> **Note on `pip install --user`.** If `mido` lives in `~/.local/lib/python3.X/site-packages`, it is only importable when `HOME` is set to that user. Anything that launches Padfire with a different or empty `HOME` (some sandboxes, some service managers) will fail at `import mido`. Prefer distro packages for a system service, or set `PYTHONPATH` explicitly.

---

## Launch

```bash
./padfire-launch        # recommended -- silences JACK error spam before import
python3 padfire.py      # direct launch (JACK may print errors on startup)
```

`padfire-launch` is a shell wrapper that pre-loads `libjack.so.0` and overrides its error/info callbacks before Python imports `mido`, preventing the JACK "cannot connect" spam that appears even when JACK is not in use.

If no X or Wayland display is available, Padfire exits with a one-line message instead of a GTK traceback.

---

## Command line

```bash
./padfire-launch --show     # show existing instance
./padfire-launch --stop     # stop all audio
./padfire-launch --reload   # re-read padfire.json without restarting
./padfire-launch --quit     # ask a running instance to exit
./padfire-launch --status   # JSON status for Hearth/tools
./padfire-launch --version  # print version
./padfire-launch --install  # install a systemd --user service
```

`--status` **always** prints valid JSON, so it is safe to pipe into a parser. With no instance running it prints `{"running": false, "version": "..."}` and exits 0.

`--show`, `--stop`, `--reload` and `--quit` print human-readable text and exit **1** when no instance was listening, so scripts can tell "done" from "nothing was there".

---

## Config

Pad assignments are saved to `~/.config/padfire/padfire.json`. Edit via the GUI (right-click a pad, or use the assignment dialog).

Saves are atomic and keep one generation of backup at `padfire.json.bak`. A corrupt or unreadable config does not prevent startup -- Padfire falls back to defaults rather than refusing to open.

Configs are migrated forward automatically on load (`CONFIG_VERSION`), and unknown keys from older versions are pruned rather than carried along.

---

## MIDI mapping

| Input | Action |
|---|---|
| CC 104-111 (top row) | Switch to page 1-8 |
| Note on (grid) | Play pad sound / stop if looping |
| Note off | -- |
| Side column notes | Assignable (same as grid pads) |

Grid note numbers are `row * 16 + col`; side-column notes are `row * 16 + 8`.

LED protocol: `velocity = (green & 3) << 4 | (red & 3) | 0x0C`

The `0x0C` term selects double-buffered display without a CC 0 reset. Every LED constant in `padfire.py` satisfies this expression.

---

## Keyboard (window)

| Key | Action |
|---|---|
| `1`-`8` (or keypad) | Switch to page 1-8 |
| `Space` | Stop all audio |
| `Esc` | Hide to tray (minimize, if the tray is unavailable) |
| `Ctrl+Q` | Quit |

---

## Tools

`tools/padfire-shot` -- open the window, let it settle, capture a PNG, exit. Intended for UI review, bug reports and automated visual checks; it never touches your real config when run with `--demo`.

```bash
tools/padfire-shot                       # 1s settle, 60% centre crop -> /tmp/padfire-shot.png
tools/padfire-shot --demo                # seed a throwaway HOME with example pads
tools/padfire-shot --demo --scale 250     # also write a 250% upscale
tools/padfire-shot --playing 0:1          # render a pad in its playing state
tools/padfire-shot -o out.png --crop 100  # full window, no crop
```

It writes the capture plus any crop/scale variants and prints each path with its byte size and sha256, so a screenshot can be verified rather than trusted. MIDI, IPC, VU and tray are stubbed out during capture, so it is safe to run while a real instance is live.

Run the fast, hardware-free v2.3 regression checks with:

```bash
python3 tests/test_padfire_core.py
```

These checks use a temporary `HOME`; they cover config migration and backup rotation, malformed-config fallback, configured VU-source selection, Launchpad Mini MK3 rejection, and LED protocol invariants. They do not replace a separate physical-device/audio playtest.
