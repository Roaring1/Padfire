# Behavioral compatibility and acceptance matrix

The released Linux v2.4.0 implementation is the primary reference. The
unmerged Windows PR #2 is a secondary reference for Windows-specific behavior
that must be brought into the shared Rust implementation.

| Area | Required behavior | Proof before cutover |
|---|---|---|
| Config | v1-v5 JSON load, v5 save, atomic replace, previous-generation backup, 4 MiB bound, private ownership/permissions, corrupt-config fallback | Golden fixtures, migration tests, symlink/hardlink/special-file/race tests on each OS |
| Config semantics | 8 active pages, per-pad action/file/sink/0-150% volume/loop/label/color/script/PID/stop target/retrigger/notes | Round-trip tests against representative legacy fixtures |
| MIDI input | Note-on, velocity-zero note-on, note-off, CC 104-111; side column maps to note row*16+8 | Deterministic packet tests and hardware acceptance |
| Device safety | Mini MK1/MK2 only; MK3 rejected before output port opens; no speculative writes | Enumeration mocks, explicit MK3 negative test, physical MK1/MK2 |
| MIDI lifecycle | startup retry, partial-open cleanup, detect port disappearance, disconnect status, reconnect without reopening UI, serialized LED writes, shutdown joins workers | State-machine tests, simulated unplug/replug, real USB unplug/replug |
| LEDs | Existing values and mandatory double-buffer bits; assigned/empty/playing/stop/script/page colors; clear safely on shutdown | Exhaustive encoding assertions and physical check |
| Audio | File decoding, multiple simultaneous sounds, per-pad sink, global sink, default sink fallback, 0-150% volume, loop, restart/toggle retrigger, Stop, Stop All | Fake backend tests, audio fixture replay and actual sound check |
| Monitor | Optional independent second output; do not duplicate when target equals primary; monitor-sink selection; VU from configured monitor/default output | Routing matrix tests, live PipeWire/WASAPI playback/capture |
| VU | RMS display; preserve PCM frame alignment; stop capture when idle; bounded retry/backoff; no idle polling/spawn loops | Synthetic PCM tests, idle resource test, live capture |
| Script pads | Explicit user-authored command; one owned process per pad; per-pad retrigger; bounded timeout; terminate process tree; reap; Test Run cancellable | Fake child process tests on Linux and Windows |
| Crosshair action | Resolve executable on PATH, then supported user-local fallback; call --toggle using argv, no shell, off callback thread, 5-second timeout and clear error | Fake executable tests on both OSs |
| Stop actions | Stop SFX, Stop All, stop target pad, stop scripts and sounds, invalidate pending starts before asynchronous cleanup | Race-focused deterministic concurrency tests |
| UI | Compact dark UI, 8-page selector, 8x8 pad grid plus side column, editable labels/colors, clear, right-click context menu, file drop, pad drag/drop | Headless UI tests plus visible Linux/Windows acceptance |
| Pad editor | Play/script/stop actions, file/command browse, Test Run, volume, loop, per-pad retrigger, sink, loudness analysis, PID file, notes | UI state tests and manual operation |
| Settings | Global output, global second-press mode, monitor enable/output, persistence | UI and round-trip tests |
| Window/tray | Window close hides to tray when supported, Esc hides, keyboard 1-8/keypad page select, Space Stop All, Ctrl+Q quit, show/stop/quit tray menu | Native event tests, actual tray click test on KDE and Windows |
| IPC/CLI | status JSON even if stopped; show/stop/reload/quit return failure when no instance; fixed commands only | Golden JSON, framing/size/deadline and one-instance tests |
| Packaging | One Windows and one Linux binary family, app icon/desktop entry, no auto-start, clean upgrade/rollback, third-party notices | Clean VM/runner install-upgrade-uninstall tests |
| Security | No network listener; private config; safe paths; explicit script trust boundary; no shell for Crosshair; child process cleanup | Threat-model review, static analysis, tests, CodeQL/dependency audit |

## Baseline recorded before the Rust rewrite

On 2026-11-20, before any source changes on this branch:

- Linux core suite: **83/83 passed**.
- GTK dialog suite under Xvfb: **7/7 passed**.
- Live read-only status: v2.4.0 running, Launchpad Mini connected, page 0,
  no active sounds, monitor enabled on the configured vm_music sink.

No real audio playback, MIDI output, pad press, config replacement, or live
instance restart was performed during this baseline.
