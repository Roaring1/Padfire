# Padfire

Padfire is a soundboard for the Novation Launchpad Mini MK1/MK2. It provides
eight pages of assignable pads, sound playback, LED feedback, per-pad actions,
and a keyboard/mouse interface.

The repository now contains two native desktop front ends over the same
configuration and Launchpad MIDI contract:

- **Linux:** the established GTK3 application using PipeWire/PulseAudio.
- **Windows:** a native Qt application using Windows audio endpoints and the
  Windows MIDI backend. It does not require WSL, MSYS2, PipeWire, GTK, or a
  Unix shell.

The Linux application remains available as **padfire.py**; the Windows entry
point is **padfire_windows.py**.

---

## Features

- 8 pages, 64 grid pads and 8 side-column pads per page
- Top-row Launchpad CC 104–111 switches pages 1–8
- Per-pad sound file, label, volume (0–150%), output device, loop, retrigger
  mode, color and notes
- Per-pad actions: **play**, **script**, **stop_sfx**, **stop_all**, **stop_key**
- Pad LED feedback uses the Launchpad Mini MK1/MK2 double-buffer protocol
- Missing sound files are visibly marked on the pad
- Drag audio files onto pads; drag pads to move them, or Ctrl-drag to copy
- Tray controls for show, stop-all, settings and quit
- IPC commands: **status**, **stop_all**, **show**, **reload**, **quit**
- MIDI input auto-reconnects after the Launchpad is unplugged/reconnected
- Loudness analysis suggests a pad volume for a -18 LUFS target
- Shared version-5 JSON config, atomic saves and one previous-generation backup

**Supported hardware:** Launchpad Mini MK1 and MK2. MK3 is intentionally
excluded until its different MIDI/LED protocol is implemented and tested.

---

## Windows

### Requirements

- Windows 10 or 11 (64-bit)
- Python 3.12 (64-bit) for a source checkout
- Novation Launchpad Mini MK1 or MK2
- A Windows playback device (WASAPI/shared mode through PortAudio)

The Windows app uses PySide6 for its desktop UI, python-rtmidi for MIDI,
and sounddevice / soundfile for audio playback and decoding. The selected
output is a Windows audio endpoint; an old Linux sink name found in an imported
config safely falls back to the Windows system default until changed in
Settings.

### Run from source

Open PowerShell in the repository:

    py -3.12 -m venv .venv
    .\.venv\Scripts\Activate.ps1
    python -m pip install --upgrade pip
    python -m pip install -r packaging/windows/requirements.txt
    python .\padfire_windows.py

The first start creates the config at:

    %APPDATA%\Padfire\padfire.json

Your existing version-5 config can be copied there. Pad assignments, labels,
actions, colors, loop/retrigger settings and notes are shared. Sound paths are
not rewritten during import: relink files whose Linux paths do not exist on
Windows, and choose Windows output devices in Settings.

### Windows commands

    python .\padfire_windows.py --version
    python .\padfire_windows.py --status
    python .\padfire_windows.py --show
    python .\padfire_windows.py --stop
    python .\padfire_windows.py --reload
    python .\padfire_windows.py --quit
    python .\padfire_windows.py --install
    python .\padfire_windows.py --uninstall

The install command adds a per-user Windows sign-in entry under
HKCU\Software\Microsoft\Windows\CurrentVersion\Run; it does not request
administrator access. Uninstall removes only that Padfire entry.

In the installed build, use the console companion for commands and scripting:

    .\CLI\PadfireCLI.exe --status
    .\CLI\PadfireCLI.exe --show
    .\CLI\PadfireCLI.exe --stop
    .\CLI\PadfireCLI.exe --reload
    .\CLI\PadfireCLI.exe --quit
    .\CLI\PadfireCLI.exe --install
    .\CLI\PadfireCLI.exe --uninstall

### Build the Windows executable

    .\packaging\windows\build.ps1 -Clean

This runs syntax checks, the cross-platform contract tests, an offscreen Qt UI
smoke test and a PyInstaller build. It creates two one-folder builds: the windowed UI at dist\Padfire\Padfire.exe
and the console control utility at dist\PadfireCLI\PadfireCLI.exe. Qt shared
libraries remain separate beside each executable rather than being hidden in
one opaque file.

To also produce a per-user installer, install Inno Setup and run:

    .\packaging\windows\build.ps1 -Installer

The installer definition is packaging/windows/Padfire.iss. It installs
under the current user's LocalAppData, so no administrator account is needed.
The one-folder layout keeps Qt DLLs separate and replaceable. Before publishing
a binary, review THIRD_PARTY_NOTICES.md and satisfy the selected PySide6/Qt
licensing option; the PyPI distribution offers LGPL, GPL and commercial terms.

### Windows-specific notes

- The output meter follows Padfire's own audio streams. It is not a capture of
  every sound playing elsewhere on the Windows desktop.
- Hearth integration is not enabled in the Windows app because the current
  Hearth service exposes a Linux Unix-domain-socket interface. Padfire itself
  runs without Hearth.
- .cmd and .bat commands run through the Windows command processor. Direct
  .ps1 files are started with PowerShell and respect the user's execution
  policy; Padfire does not silently bypass it.
- Hardware-free CI does not prove a physical Launchpad, speaker, USB hub or
  audio driver works. Test those on the target Windows PC before relying on it.

---

## Linux

### Requirements

- Python 3.10+
- GTK3 / PyGObject
- mido + python-rtmidi
- paplay (PipeWire / pipewire-pulse)
- parec (optional; VU meter hides itself if absent)
- libappindicator3 (optional tray backend)
- Novation Launchpad Mini MK1 or MK2

Fedora / Nobara / RHEL-like:

    sudo dnf install python3-gobject python3-mido python3-rtmidi libappindicator-gtk3

Arch:

    sudo pacman -S python-gobject python-mido python-rtmidi libappindicator-gtk3

If distro MIDI bindings are unavailable or too old:

    pip install --user mido python-rtmidi

### Launch and commands

    ./padfire-launch
    python3 padfire.py
    ./padfire-launch --show
    ./padfire-launch --stop
    ./padfire-launch --reload
    ./padfire-launch --quit
    ./padfire-launch --status
    ./padfire-launch --version
    ./padfire-launch --install

The launcher resolves padfire.py next to itself and preloads the JACK
callback shim before importing MIDI libraries. The Linux install command
writes and enables a systemd --user service.

---

## Configuration and MIDI mapping

Linux config: ~/.config/padfire/padfire.json (or
$XDG_CONFIG_HOME/padfire/padfire.json).

Windows config: %APPDATA%\Padfire\padfire.json.

Both front ends preserve the same version-5 schema. Saves are atomic and keep
one previous generation at padfire.json.bak. An unreadable config falls
back to safe defaults; the file remains available for recovery.

| Input | Action |
|---|---|
| CC 104–111 (top row) | Switch to page 1–8 |
| Note on (grid) | Play pad sound / stop if looping |
| Note off | Release / clear press feedback |
| Side-column notes | Assignable pads, same as grid |

Grid note numbers are row * 16 + col; side-column notes are row * 16 + 8.

LED protocol: velocity = (green & 3) << 4 | (red & 3) | 0x0C.
The 0x0C term selects double-buffered display without a CC 0 reset.

## Keyboard controls

| Key | Action |
|---|---|
| 1–8 (or keypad) | Switch page |
| Space | Stop all audio |
| Esc | Hide to tray (or minimize without a tray) |
| Ctrl+Q | Quit |

---

## Tests and validation

Linux regression checks:

    python3 tests/test_padfire_core.py

Linux CI also compiles the app and launcher, runs ShellCheck, and renders the
actual GTK UI under Xvfb.

Windows contract tests and offscreen UI smoke test:

    python -m unittest discover -s tests -p "test_padfire_platform.py"
    $env:QT_QPA_PLATFORM = "offscreen"
    python -m unittest discover -s tests -p "test_padfire_windows_ui.py"
    python .\padfire_windows.py --smoke-ui

The automated suites do not open MIDI hardware or play to real output devices.
Do a physical Launchpad/audio playtest on each platform before making a release.
