# Padfire

GTK3 soundboard for the Novation Launchpad Mini (MK1/MK2).

8 pages of 8x8 pads. Each pad plays a sound file via PipeWire (`paplay`). LED feedback with per-page colors. IPC socket so Hearth can show playback status.

---

## Features

- 8 pages (top-row CC 104-111), 64 assignable pads per page
- Per-pad: sound file, label, volume (0-150%), output sink, loop toggle, color
- LED feedback: pad lights on press, color reflects assignment state
- Tray icon with page indicator and stop-all
- IPC: `status`, `stop_all`, `show` commands via Unix socket (`~/.config/padfire/padfire.sock`)
- Hearth integration: Padfire status panel in Hearth shows current page, playing sounds, MIDI connection state
- MIDI input via `mido` (rtmidi backend), auto-reconnect on device unplug/replug
- Audio via `paplay` subprocess -> PipeWire, no JACK/PortAudio dependency

---

## Requirements

- Python 3.10+
- GTK3 (`python-gobject`)
- `mido` + `python-rtmidi`
- `paplay` (PipeWire / pipewire-pulse)
- `libappindicator3` (tray, optional)
- Novation Launchpad Mini MK1 or MK2

```bash
# Arch
sudo pacman -S python-gobject python-mido python-rtmidi libappindicator-gtk3
```

---

## Launch

```bash
./padfire-launch        # recommended -- silences JACK error spam before import
python3 padfire.py      # direct launch (JACK may print errors on startup)
```

`padfire-launch` is a shell wrapper that pre-loads `libjack.so.0` and overrides its error/info callbacks before Python imports `mido`, preventing the JACK "cannot connect" spam that appears even when JACK is not in use.

---

## Command line

```bash
./padfire-launch --show     # show existing instance
./padfire-launch --stop     # stop all audio
./padfire-launch --status   # JSON status for Hearth/tools
./padfire-launch --version  # print version
```

---

## Config

Pad assignments saved to `~/.config/padfire/padfire.json`. Edit via the GUI (right-click a pad or use the assignment dialog).

---

## MIDI mapping

| Input | Action |
|---|---|
| CC 104-111 (top row) | Switch to page 1-8 |
| Note on (grid) | Play pad sound / stop if looping |
| Note off | -- |
| Side column notes | Assignable (same as grid pads) |

LED protocol: `velocity = (green & 3) << 4 | (red & 3) | 0x0C`

---

## Keyboard (window)

| Key | Action |
|---|---|
| `Esc` | Hide to tray |
| `Ctrl+Q` | Quit |
