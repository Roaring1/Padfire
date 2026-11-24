# Padfire behavior and acceptance matrix

The native Rust workspace is the only active implementation. The former GTK/Python v2.4.0 behavior is represented by compatibility fixtures and documented expectations, not by a second executable or GTK test suite.

| Area | Required behavior | Automated proof | Remaining manual acceptance |
|---|---|---|---|
| Config | v1-v5 JSON load, v5 save, atomic replace, previous-generation backup, 4 MiB bound, private ownership/permissions, corrupt-config fallback | Golden fixtures, migration tests, symlink/hardlink/special-file/race tests on each OS | Verify existing v5 assignments remain intact on the target machine |
| Config semantics | 8 active pages, per-pad action/file/sink/0-150% volume/loop/label/color/script/PID/stop target/retrigger/notes | Round-trip compatibility tests against representative legacy fixtures | Confirm real pad map only when a user requests changes |
| MIDI input | Note-on, velocity-zero note-on, note-off, CC 104-111; side column maps to note row*16+8 | Deterministic packet tests and hardware-free device selection | Physical controller input |
| Device safety | Mini MK1/MK2 only; MK3 rejected before output port opens; no speculative writes | Enumeration mocks, explicit MK3 negative test | Physical MK1/MK2 behavior |
| MIDI lifecycle | startup retry, partial-open cleanup, port disappearance, disconnect status, reconnect without reopening UI, serialized LED writes, shutdown joins workers | State-machine tests and simulated unplug/replug | Real USB unplug/replug |
| LEDs | Existing values and mandatory double-buffer bits; assigned/empty/playing/stop/script/page colors; clear safely on shutdown | Exhaustive encoding assertions | Physical LED output |
| Audio | File decoding, multiple simultaneous sounds, per-pad sink, global sink, default fallback, 0-150% volume, loop, restart/toggle retrigger, Stop, Stop All | Fake backend tests, audio fixtures and engine tests | Actual playback/routing on PipeWire and WASAPI |
| Monitor/VU | Optional independent second output, monitor-sink selection, RMS capture, idle stop and bounded retry | Synthetic PCM, routing and idle-resource tests | Real loopback/VU behavior |
| Script pads | Explicit user-authored command, one owned process per pad, retrigger, bounded timeout, terminate process tree, reap, cancellable test run | Fake child-process tests on Linux and Windows | Review actual trusted scripts before running them |
| Crosshair action | Resolve canonical executable from PATH, launch fixed --toggle argument without a shell, off callback thread | Fake executable tests on both OSs | Optional real Crosshair toggle |
| Stop actions | Stop SFX, Stop All, stop target pad, stop scripts and sounds, invalidate pending starts before asynchronous cleanup | Race-focused deterministic concurrency tests | Manual response check with real audio |
| UI | Native window, 8-page selector, 8x8 pad grid plus side column, editor/settings, tray, keyboard shortcuts and drag/drop | Headless Xvfb render and CLI/UI tests | Visible KDE Wayland and Windows UI/tray |
| IPC/CLI | Status JSON even if stopped; show/stop/reload/quit return failure when no instance; fixed commands only | Golden JSON, framing/size/deadline and one-instance tests | Verify desktop-session behavior |
| Installer | One per-user application entry, icon, managed binary and third-party notice; config preserved on uninstall | Install/upgrade/uninstall lifecycle and license-byte tests | Release provenance/signing |
| Security | No network listener; private config; safe paths; explicit script trust boundary; no shell for Crosshair; child cleanup | Threat-model review, tests, CodeQL/dependency audit | Monitor findings as code evolves |

## Current verification

The full local workspace suite contains 144 Rust tests and passed with formatting, strict Clippy and optimized release build. Hosted Ubuntu and Windows matrices, dependency/license policy, Linux UI rendering and the hardware-free Rust smoke tests passed. The active CodeQL workflow scans Rust and Actions with the extended security query suite; the retired Python application is no longer a scanning target.

Automated tests do not replace physical Launchpad, real audio, Windows UI or signing/provenance acceptance.
