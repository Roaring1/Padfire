# Changelog

## Unreleased

- **Standalone, denser main window.** Removed the permanent bottom status bar
  and all Hearth-specific status widgets/polling. The VU meter now sits
  directly beneath the Padfire wordmark at the top-left; live playback names
  and transient errors use the existing utility row. The pad grid remains the
  focus.
- **Standalone runtime.** Removed the Hearth socket probe and companion-window
  button. Padfire keeps its small, same-user Unix-socket API for optional
  external integrations, but no longer requires or probes for a companion app.
- **Portable KDE/GTK application identity.** Bundled the existing scalable
  Padfire SVG, installs it into the user's hicolor icon theme, sets the desktop
  entry's `Icon=padfire` and `StartupWMClass=padfire`, and explicitly sets the
  GTK/GDK program identity. The installer updates an existing autostart entry
  without creating a new one.
- **Portable audio defaults.** Removed the `vm_game` and specific Astro headset
  defaults. New installs use the active PipeWire/PulseAudio default sink; an
  explicit saved sink is kept when available and falls back to the user's
  default when it is not. Both sink selectors include `System default`. The
  user service also orders startup after `pipewire-pulse.service` when present,
  so `paplay` does not race the PulseAudio compatibility server.
- **Low idle overhead.** The VU worker sleeps at 250 ms while idle and performs
  no sink discovery or capture subprocess work until Padfire is playing.
  System-default sink discovery is cached for five seconds while active.
- **Compact, readable dialogs.** Edit Pad now sizes itself to the selected
  action, uses a short path field with Browse, keeps volume at a deliberate
  width with a live percentage, and moves optional notes behind a collapsed
  expander. Settings dropdowns are bounded, labels are readable, and monitor
  switches keep their native width instead of stretching across the row.
- **Added:** a built-in `Toggle Crosshair` pad action. It invokes the installed
  Crosshair CLI without a shell, off the MIDI/GTK callback thread, and reports
  command errors in the Padfire status area. Existing pad assignments are
  unchanged.

### Fixed

- **Script processes could outlive their pad or Padfire.** Stop, Stop All,
  retrigger, and Quit now invalidate pending script launches, terminate and reap
  registered processes, and signal the script's dedicated process group so
  child commands do not remain orphaned. A stale worker cannot remove a newer
  process from the pad registry.
- **VU capture could outlive Padfire.** The level-meter worker now exits on
  shutdown, terminates and reaps `parec`, and closes its output pipe on
  shutdown, EOF, sink changes, and read errors. Partial 16-bit PCM reads retain
  their trailing byte instead of losing sample alignment.
- **Background service probes kept running during Quit.** The Hearth polling
  worker now has an interruptible shutdown wait and is joined during exit.
- **MIDI workers could linger after Quit.** The reconnect wait is now
  interruptible, initial/retry connects recheck shutdown state, and the reader
  and watcher threads are joined after the Launchpad ports close.
- **The editor's Test Run could leave child commands behind.** Test commands now
  run in a dedicated process group, stop when the dialog closes, and terminate
  descendants on timeout.
- **Modal workers outlived the pad editor.** LUFS analysis now cancels and reaps
  its `ffmpeg` process group; preview playback snapshots UI values on the GTK
  thread and cannot start after Stop or dialog close.

## v2.3

Audit-driven release. Every item below was reproduced before it was changed and
re-checked afterwards; the behavioural suite covers the LED protocol, config
migration, save/backup round-trips, VU source selection, missing-file detection
and the CLI surface.

### Fixed

- **Startup crash.** `Gdk.Screen.get_default()` can return `None` when no
  display is usable, which raised `TypeError: Argument 0 does not allow None as
  a value` from `_build_ui`. Padfire now exits with a single clear line.
- **Monitor button was bright pink.** With `monitor_enabled: true` the Mon
  button borrowed the status-label class `.stat-ok`, so it rendered hot pink on
  every launch. It now uses real toggle styling (`.act` / `.act-on`).
- **Orphaned Hearth readouts.** With Hearth offline the status bar showed three
  bare letters `G  C  M` with no values. The whole group is hidden when there is
  nothing to report.
- **VU meter ignored your config.** It read a hardcoded `vm_game.monitor`
  instead of the configured monitor/output sink, and spawned `parec`
  unconditionally. It now derives the source from config, checks that `parec`
  exists, backs off after failure, reaps dead children, and restarts when the
  sink changes.
- **Pad labels truncated mid-word** (`clicker s`, `clicker t`). Labels now
  wrap to two lines with proper ellipsis.
- **`--status` was not JSON.** With no instance running it printed the bare text
  `not running` and exited 0, so every consumer of the documented "JSON status
  for Hearth/tools" raised on the first character. It now always emits valid
  JSON.
- **Silent success from action flags.** `--show`, `--stop`, `--reload` and
  `--quit` exited 0 even when nothing was listening. They now exit 1.
- **Side-column pads rendered as ovals.** They were 30px wide with a 15px
  corner radius, so the radius consumed the entire half-width. They now match
  the grid's 6px radius.
- **Red meant two different things.** A missing sound file and a stop-action pad
  were both solid red. Missing files now use a dashed border, which reads as
  "broken" independently of colour.
- **`install_service()`** hardcoded `Environment=WAYLAND_DISPLAY=wayland-0`,
  guessed the launcher path, and shelled out through `os.system`. It now
  resolves the launcher relative to the script, adds
  `PartOf=graphical-session.target`, and uses `subprocess.run`.
- **Config saves no longer risk having no file at all.** The previous contents
  are copied to `padfire.json.bak` rather than the primary being renamed away.
- **A corrupt config no longer blocks startup** -- Padfire falls back to
  defaults instead of refusing to open.
- **Tray page indicator was a stub.** It now shows `P1`-`P8`.
- **Every bare `except:` is gone** (19 of them). `grep -cE 'except:'` reports 0.

### Added

- **Missing-file pad state.** A pad whose sound file no longer exists is marked
  in the grid before you press it.
- **Keyboard pages.** `1`-`8` (and the keypad equivalents) switch pages;
  `Space` stops all audio.
- **`tools/padfire-shot`** -- opens the window, lets it settle, captures a PNG,
  exits. Prints byte size and sha256 for each file it writes so a screenshot
  can be verified rather than trusted. Stubs MIDI, IPC, VU and the tray, so it
  is safe to run alongside a live instance.
- **README rewritten** against the actual host: the install line targeted Arch
  (`pacman`) on a Fedora-derived system, and several shipped flags, IPC verbs
  and keybindings were undocumented.

## 2.2 -- 2026-06-21

### Fixed

- Sink dropdowns now select the intended hardware sink when the list contains
  the "-- hardware --" separator. The previous indexing skipped over the
  separator, so the saved sink could appear one row off.
- Side-column pads now support the same drag-to-move / Ctrl-drag-to-copy
  behavior as grid pads.
- Failed file drops now finish the GTK drag operation cleanly instead of
  leaving the drag source in an ambiguous state.
- "Stop a specific pad", "Stop this pad", and clearing a pad now stop script
  pads as well as audio pads; script pads now also appear in the stop-target
  picker.
- README config/socket paths now point to `~/.config/padfire` instead of the
  legacy `~/.config/roaring` location.

### Changed

- Packaging examples are less machine-specific: desktop launcher uses
  `padfire-launch` + a theme icon, systemd uses `%h/bin/padfire-launch`, and
  the restart helper resolves the repo root with environment-variable
  overrides.

## 2.1 -- 2026-06-21

### Fixed

- Looping pads now actually loop. The playback thread removed its own entry
  from the active-process table and then checked whether that entry still
  existed to decide whether to repeat -- which was always false, so a looped
  sound played exactly once. It now distinguishes a natural end (repeat) from
  an external Stop (don't repeat).
- Pad colors no longer linger after switching pages. Switching pages dropped
  the table that tracked each pad's color styling without removing the old
  styling from the buttons, so a colored pad on one page could keep its color
  on another page where the slot was empty or a different color. Color styling
  is now reconciled per pad on every switch.

### Changed

- Pad color styling is only rebuilt when a pad's color actually changes,
  instead of recreating a CSS provider for every colored pad about five times
  a second from the refresh loop.
- Config saves are now atomic and keep a single rolling backup
  (`padfire.json.bak`), so an interrupted save can't corrupt the file that
  holds every pad assignment.
- `padfire-launch` now finds `padfire.py` next to itself (overridable via
  `$PADFIRE_PY`) instead of the hardcoded `/home/roaring/bin/padfire.py`
  path, so it runs from the repo or any install location.
- `extra-scripts/padfire-restart.sh`: corrected the leftover socket path
  (`~/.config/roaring` -> `~/.config/padfire`).

### Added

- Keyboard shortcuts on the main window: `Esc` hides to tray, `Ctrl+Q` quits.
- `--version` flag on the command line.

### Removed

- Removed a dead, shadowed copy of the Hearth-status update method (a later
  definition always overrode it).
