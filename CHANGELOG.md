# Changelog

## 2.2 — 2026-06-21

### Fixed
- Sink dropdowns now select the intended hardware sink when the list contains the "-- hardware --" separator. The previous indexing skipped over the separator, so the saved sink could appear one row off.
- Side-column pads now support the same drag-to-move / Ctrl-drag-to-copy behavior as grid pads.
- Failed file drops now finish the GTK drag operation cleanly instead of leaving the drag source in an ambiguous state.
- "Stop a specific pad", "Stop this pad", and clearing a pad now stop script pads as well as audio pads; script pads now also appear in the stop-target picker.
- README config/socket paths now point to `~/.config/padfire` instead of the legacy `~/.config/roaring` location.

### Changed
- Packaging examples are less machine-specific: desktop launcher uses `padfire-launch` + a theme icon, systemd uses `%h/bin/padfire-launch`, and the restart helper resolves the repo root with environment-variable overrides.

## 2.1 — 2026-06-21

### Fixed
- Looping pads now actually loop. The playback thread removed its own entry from the active-process table and then checked whether that entry still existed to decide whether to repeat — which was always false, so a looped sound played exactly once. It now distinguishes a natural end (repeat) from an external Stop (don't repeat).
- Pad colors no longer linger after switching pages. Switching pages dropped the table that tracked each pad's color styling without removing the old styling from the buttons, so a colored pad on one page could keep its color on another page where that slot was empty or a different color. Color styling is now reconciled per pad on every switch.

### Changed
- Pad color styling is only rebuilt when a pad's color actually changes, instead of recreating a CSS provider for every colored pad about five times a second from the refresh loop.
- Config saves are now atomic and keep a single rolling backup (`padfire.json.bak`), so an interrupted save can't corrupt the file that holds every pad assignment.
- `padfire-launch` now finds `padfire.py` next to itself (overridable via `$PADFIRE_PY`) instead of the hardcoded `/home/roaring/bin/padfire.py` path, so it runs from the repo or any install location.
- `extra-scripts/padfire-restart.sh`: corrected the leftover socket path (`~/.config/roaring` → `~/.config/padfire`).

### Added
- Keyboard shortcuts on the main window: `Esc` hides to tray, `Ctrl+Q` quits.
- `--version` flag on the command line.

### Removed
- Removed a dead, shadowed copy of the Hearth-status update method (a later definition always overrode it).
