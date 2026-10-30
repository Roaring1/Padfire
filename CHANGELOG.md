# Changelog

## Unreleased — Windows port (2.4-dev)

### Added
- A dependency-light padfire_core.py for shared version-5 config migration,
  atomic saves, backup rotation and platform-aware user paths.
- A native Windows Qt front end with the full pad action model, 8x8 + side
  column layout, pages, context editing, drag/copy/drop, tray controls and
  keyboard shortcuts.
- Windows MIDI through mido / python-rtmidi, retaining the existing
  Launchpad Mini MK1/MK2 note, CC and LED protocol.
- Windows audio playback through sounddevice and soundfile, with device
  enumeration, per-pad volume, looping, monitor duplication and an output
  meter for Padfire playback.
- Windows named-pipe IPC, a per-user single-instance mutex, a windowed GUI
  executable plus a separate console control executable, per-user Registry
  startup registration, PyInstaller build metadata and an Inno Setup installer.
- Separate cross-platform contract tests and an offscreen Qt smoke test.
- A Windows GitHub Actions job that validates the UI and packages the executable.

### Preserved
- The existing Linux GTK/PipeWire application, launcher, tray behavior,
  systemd --user install path and current Linux tests remain available.
- Existing version-5 assignments remain readable by both front ends. The
  Windows app does not rewrite the user's stored Linux device names or sound
  paths merely because those resources are unavailable on Windows.

### Known platform differences
- The Windows output meter measures Padfire playback streams, not the entire
  Windows desktop mix.
- The GUI executable writes diagnostics to a rotating log under
  %LOCALAPPDATA%\Padfire\Logs; use PadfireCLI.exe for visible CLI output.
- The current Hearth companion interface is Linux Unix-socket based and is
  not shown as connected by the Windows front end.
- A physical Launchpad/audio playtest remains a release gate; CI is hardware-free.

## v2.3

Audit-driven release. Every item below was reproduced before it was changed and
re-checked afterwards; the behavioural suite covers the LED protocol, config
migration, save/backup round-trips, VU source selection, missing-file detection
and the CLI surface.

### Fixed

- **Startup crash.** Gdk.Screen.get_default() can return None when no
  display is usable, which raised a GObject error from _build_ui. Padfire now
  exits with a clear message.
- **Monitor button styling.** The toggle now uses the correct active button style.
- **Hearth readouts.** The status group is hidden when Hearth is offline.
- **VU source.** The meter follows the configured monitor/output sink, checks
  for parec, backs off after failure and reaps dead children.
- **Missing file indication.** A pad whose sound file no longer exists is
  marked in the grid before it is pressed.
- **Config reliability.** Saves remain atomic with a backup, and corrupt config
  falls back to defaults rather than blocking startup.
- **CLI contracts.** --status always emits valid JSON; action commands return
  a nonzero code when no instance answers.
- **Tray page indicator.** The tray title now displays P1–P8.
- Launchpad Mini MK3 is rejected from the legacy MK1/MK2 protocol matcher.

### Added

- tools/padfire-shot for GTK UI screenshots with MIDI, IPC, VU and tray side
  effects disabled.
- Hardware-free config, VU, device matcher and LED protocol regression tests.

See the Git history for the complete v2.3 audit notes and earlier changes.
