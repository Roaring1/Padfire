# Architecture decisions

Status: accepted for implementation unless a later decision record supersedes
the choice.

## ADR-001 - One native Rust application for Linux and Windows

**Decision:** Use a single Rust workspace, one UI codebase and thin
platform-specific adapters. Do not maintain separate GTK and Qt application
front ends.

**Why:** Padfire is a small, latency-sensitive desktop utility. A webview
stack would add a browser runtime; separate GTK/Qt front ends would duplicate
product behavior and testing. The native Rust UI must run with no Python,
PySide, Qt, webview or Node runtime.

**Initial UI candidate:** eframe/egui 0.37.x, with its native window backend
and the OpenGL glow renderer. eframe is MIT/Apache-2.0, runs natively on Linux
and Windows, and supports custom compact controls. The renderer choice is
provisional until the target Nobara Wayland session and Windows laptop have
both passed visible UI, resize, sleep/resume and resource tests. WGPU is not
enabled by default for this utility unless validation shows a concrete need.

**Rejected for this port:** PySide/Qt, because it keeps the two-platform UI
split and has a substantial binary/plugin/source-offer/relinking audit trail;
Tauri, because a webview is unnecessary overhead; Slint, because its GPL or
commercial licensing choice is avoidable for this MIT application; two
different native GUI implementations, because behavior would drift.

## ADR-002 - Keep the existing JSON config schema during the port

**Decision:** Read/write v5 and preserve unrecognized root and pad properties.
The Rust app performs a guarded one-time import from the historical
~/.config/roaring/padfire.json location only when no Rust config exists.

**Why:** Retiring the GTK/Python executable does not justify discarding user
pad assignments or breaking existing v5 configurations. A future schema
change requires an explicit migration and backup test.

## ADR-003 - Treat hardware as capability-scoped profiles

**Decision:** The initial production MIDI profile is Mini MK1/MK2 only. A
profile must explicitly own both input and output protocol behavior. Device
enumeration is not permission to send output. MK3 and unknown devices remain
unsupported/read-only.

**Why:** Mini MK3 uses a different protocol. A device-name match must not cause
MK1/MK2 LED or note messages to be sent to an incompatible controller.

## ADR-004 - Separate audio engine semantics from audio-server adapters

**Decision:** Preserve one action/playback contract while selecting an
appropriate native backend per OS. Linux must integrate with PipeWire through
its supported PulseAudio compatibility layer or native PipeWire API; Windows
must use WASAPI. Per-pad output routing, independent monitor routing, mixing,
loop/retrigger, VU capture and cancellation are first-class requirements.

**Why:** The current Linux app routes each playback to a named PipeWire/Pulse
sink through paplay; the Windows draft uses PortAudio/WASAPI devices. A common
abstraction is preferable to pretending those endpoint models are identical.
The actual backend choice is not finalized until device enumeration, default
routing, monitor capture and reconnect behavior are proven on both OSs.

## ADR-005 - No real-time audio work on the GUI or MIDI callback thread

**Decision:** The MIDI callback only validates/decodes events and forwards
bounded commands. File decoding, audio stream creation, script execution,
configuration writes, IPC clients and loudness analysis run on workers.
Audio callbacks never block on UI locks or filesystem operations.

**Why:** The released code already contains generation tokens to prevent a
late-starting audio/script worker from surviving Stop or Stop All. The Rust
design retains this invariant and avoids unbounded thread/process creation.

## ADR-006 - Preserve the local-control API, improve its transport boundary

**Decision:** Keep status, stop_all, show, reload and quit, with the existing
JSON status shape and CLI exit behavior. Linux uses an owner-only Unix socket.
Windows uses a current-user-only named pipe. Both have bounded requests,
bounded clients, deadlines, one-instance ownership and no network listener.

## ADR-007 - User-session lifecycle

**Decision:** A normal launch runs the native GUI/application; explicit quit
stops audio, MIDI, VU capture and child processes. On the Nobara deployment,
a user-scoped systemd unit starts Padfire with graphical-session.target and
runs with the logged-in user. It is not a system/root service.

## ADR-008 - Retire the legacy GTK/Python application

**Status:** Accepted; retirement completed 2026-11-24.

**Decision:** Keep historical v2.4.0 source recoverable through Git history,
but remove the GTK/Python runtime, launchers, packaging, tests and separate CI
workflow from the default branch and workstation. The native Rust workspace is
the only current implementation and installation. Preserve compatible v5
configuration parsing/migration; do not keep a second executable installed.

## ADR-009 - Model application actions as typed intent, not UI callbacks

**Decision:** UI, MIDI, tray and IPC all send the same bounded command enum to
one state owner. UI code does not call audio/MIDI/process APIs directly.

**Why:** This makes event ordering, Stop races, page-change behavior and
multi-input operation deterministic and unit-testable.


## ADR-010 - One Padfire desktop identity

**Decision:** The Rust executable remains named padfire-rust internally, but
its user-facing desktop and Start Menu identity is simply Padfire. The
Nobara workstation uses one user-scoped padfire-rust.service unit. Do not
create a duplicate legacy GTK entry or startup unit.

**Why:** There is now one active implementation. Keeping a separate “Padfire
Rust” launcher would imply two supported versions and invite accidental
launch of an obsolete runtime.
