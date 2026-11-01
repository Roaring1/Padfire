# Padfire

A compact GTK3 soundboard and macro pad app for Linux desktops. The current MIDI protocol supports the Novation Launchpad Mini MK1/MK2.

8 pages of 8x8 pads. Each pad can play a sound file through PipeWire/PulseAudio or run a local command. Padfire stores settings and assignments in the current user's home directory; it does not require a companion app or machine-specific audio sink names.

---

## Features

- 8 pages (top-row CC 104-111), 64 assignable pads per page
- Per-pad: sound file, label, volume (0-150%), output sink, loop toggle, color
- Per-pad actions: `play`, `script`, `stop_sfx`, `stop_all`, `stop_key`, `crosshair_toggle`
- LED feedback: pad lights on press, color reflects assignment state
- Missing sound files are shown on the pad itself: a dashed red border and a `!` glyph, so a broken assignment is visible before you press it
- Tray icon with a live page indicator (`P1`-`P8`) and stop-all
- IPC: `status`, `stop_all`, `show`, `reload`, `quit` over a Unix socket (`~/.config/padfire/padfire.sock`)
- A stable local Unix-socket API for optional external integrations; the standalone app works normally without another application
- New installs and missing saved audio routes use the current audio server's default output; per-pad and global sink overrides remain available
- VU meter follows your configured monitor/output sink rather than a fixed one
- MIDI input via `mido` (rtmidi backend), with automatic USB unplug/replug recovery
- A top-bar Launchpad status badge: green **Connected** or amber
  **Disconnected**. Padfire checks both MIDI ports once per second and retries
  without requiring the window to be reopened.
- Owner-only local configuration and IPC state. Padfire stores pad assignments
  in a private directory and bounds local IPC requests and concurrent clients.
- Audio via `paplay` subprocess -> PipeWire, no JACK/PortAudio dependency
- Loudness analysis (`--` LUFS target -18.0) available from the pad editor

---

## Requirements

- Python 3.10+
- GTK3 (`python-gobject`)
- `mido` + `python-rtmidi`
- `paplay` (PipeWire / PulseAudio playback CLI)
- `pactl` (recommended -- sink discovery and resolving the system-default output)
- `parec` (optional -- live VU capture; the meter hides itself if absent)
- `libappindicator3` (tray, optional)
- Novation Launchpad Mini MK1 or MK2

```bash
# Fedora / Nobara / RHEL-like
sudo dnf install python3-gobject python3-mido python3-rtmidi libappindicator-gtk3 pulseaudio-utils

# Arch
sudo pacman -S python-gobject python-mido python-rtmidi libappindicator-gtk3 libpulse

# Any distro, if the packaged MIDI bindings are missing or too old
pip install --user mido python-rtmidi
```

> **Note on `pip install --user`.** If `mido` lives in `~/.local/lib/python3.X/site-packages`, it is only importable when `HOME` is set to that user. Anything that launches Padfire with a different or empty `HOME` (some sandboxes, some service managers) will fail at `import mido`. Prefer distro packages for a system service, or set `PYTHONPATH` explicitly.

---

## Launch

```bash
./padfire-launch        # recommended -- sets up the audio environment, then launches Padfire
./padfire-launch --install  # install a per-user desktop entry, icon and systemd service
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
./padfire-launch --status   # JSON status for scripts and external tools
./padfire-launch --version  # print version
./padfire-launch --install  # install desktop icon/launcher and enable the user service
```

`--status` **always** prints valid JSON, so it is safe to pipe into a parser or consume from another app. When running, the response includes MIDI connection state, current page, active pads, and monitor settings. With no instance running it prints `{"running": false, "version": "..."}` and exits 0. The socket is local to the current user at `~/.config/padfire/padfire.sock` and is not a network service.

`--show`, `--stop`, `--reload` and `--quit` print human-readable text and exit **1** when no instance was listening, so scripts can tell "done" from "nothing was there".

---

## Config

Pad assignments are saved to `~/.config/padfire/padfire.json`. Edit via the GUI (right-click a pad, or use the assignment dialog).

Saves are atomic and keep one generation of backup at `padfire.json.bak`. A corrupt or unreadable config does not prevent startup -- Padfire falls back to defaults rather than refusing to open.

Configs are migrated forward automatically on load (`CONFIG_VERSION`), and unknown keys from older versions are pruned rather than carried along. Current versions keep `~/.config/padfire` private (0700) and the config/backup files private (0600). Config reads reject symlinks, special files and files larger than 4 MiB.

## Security and trusted configurations

Padfire's **Script** pad action intentionally executes the shell command configured by the user. Treat pad configurations as executable content: only import files from people you trust, and review script-pad assignments before triggering them. The config may also contain local file paths and device names, so redact it before sharing.

The local IPC socket is restricted to the current user and exposes a fixed set of local control/status commands; it is not a network service. See [SECURITY.md](SECURITY.md) for the security model and vulnerability-reporting guidance.

---

### Toggle Crosshair from a Launchpad pad

Right-click the pad you want to assign, choose **Edit Pad**, set **Action**
to **Toggle Crosshair**, and save. Pressing that pad runs the Crosshair CLI's
`--toggle` command, switching the overlay between enabled and disabled.
Padfire resolves `crosshair` from `PATH`, then falls back to
`~/.local/bin/crosshair`. The action runs off the GTK/MIDI callback thread
and reports missing-command, timeout, or CLI errors in Padfire's status area.
It does not change any existing pad assignments.

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

`tools/padfire-shot` -- open the window, let it settle, capture a PNG, exit. Intended for UI review, bug reports and automated visual checks; it never touches your real config when run with `--demo`. Output paths are normalized and restricted to your home directory or the system temp directory; symlink, hard-link and special-file targets are rejected.

The optional hardware smoke test at `packaging/extra-scripts/padfire_test.py`
opens the Launchpad MIDI ports, lights the grid and reports button events.
Quit Padfire first; the script refuses to claim the ports while Padfire is
running. It supports only Mini MK1/MK2, deliberately excludes MK3, and clears
the LEDs when stopped. Run it manually with:

```bash
python3 packaging/extra-scripts/padfire_test.py
```

This sends real LED messages to the controller. It is not an automated test
and should not be run while another application owns the Launchpad.

```bash
tools/padfire-shot                       # 1s settle, 60% centre crop -> /tmp/padfire-shot.png
tools/padfire-shot --demo                # seed a throwaway HOME with example pads
tools/padfire-shot --demo --scale 250     # also write a 250% upscale
tools/padfire-shot --playing 0:1          # render a pad in its playing state
tools/padfire-shot -o out.png --crop 100  # full window, no crop
```

It writes the capture plus any crop/scale variants and prints each path with its byte size and sha256, so a screenshot can be verified rather than trusted. MIDI, IPC, VU and tray are stubbed out during capture, so it is safe to run while a real instance is live.

Run the hardware-free core and GTK dialog regression checks with:

```bash
python3 tests/test_padfire_core.py
xvfb-run -a python3 tests/test_padfire_dialogs.py
```

The core checks use a temporary `HOME` and cover config migration, audio routing, MIDI discovery, disconnect detection and simulated USB unplug/replug recovery, process cleanup, and LED protocol invariants. The GTK checks exercise the dialogs under a virtual display without opening MIDI hardware or playing audio. These automated checks do not replace a separate physical-device/audio playtest.

## Roadmap and related software

The current hardware protocol is limited to Launchpad Mini MK1/MK2. Broader
generic MIDI profiles and carefully scoped USB HID input are deferred ideas,
not current features. See [ROADMAP.md](ROADMAP.md) for the far-future plan,
including explicit device-selection and protocol-safety boundaries. The
tracking issue is [#8](https://github.com/Roaring1/Padfire/issues/8).

A related but separate project is [Lunchpad](https://github.com/ruohki/lunchpad),
which supports a wider Novation Launchpad family and additional integrations.
Padfire is independent; this is a reference for ecosystem comparison, not a
dependency or compatibility claim.

## License

Padfire-authored source and documentation are licensed under the MIT License;
see [LICENSE](LICENSE). Third-party libraries, desktop runtimes and any
additional assets remain subject to their own licenses and notices.

## Project docs

- [Contributing and manual acceptance](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Future roadmap](ROADMAP.md)
- [Release checklist](RELEASING.md)
- [Release history](CHANGELOG.md)
