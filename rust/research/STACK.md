# Rust stack research

Research snapshot: 2026-11-20. Versions are candidates, not pinned production
dependencies until the native proof-of-concept and license/MSRV review pass.

## Migration method

Use v2.4.0 as a behavioral oracle. Port one subsystem at a time, generate
deterministic fixtures from safe test inputs, and require all relevant cases
to replay before switching a caller. This adopts the useful part of
rust-anything: record existing behavior, compare the Rust implementation,
then gate the change.

rust-anything is not an automatic source translator. Its app-level recorder
targets CLI tools, servers and libraries; Padfire's physical MIDI, GUI, audio
devices and OS tray need additional state-machine and manual acceptance tests.
Do not run rust-anything setup during this port because it edits developer
agent configuration and is not required for application behavior.

## GUI candidate

- eframe/egui 0.37.x: native Linux and Windows desktop framework, Rust UI,
  custom-painted controls and no embedded browser.
- Initial candidate: eframe with default features disabled, using native
  integration and the OpenGL glow renderer, plus required platform backends.
- Benchmark idle/hidden CPU, resident memory, startup, resize, Wayland/X11
  session behavior, Windows HiDPI and tray integration before locking choice.

References:
- https://github.com/emilk/egui
- https://github.com/emilk/egui/tree/master/crates/eframe
- https://docs.rs/eframe/latest/eframe/

## MIDI

- midir 0.11.1 is a Rust real-time MIDI wrapper with ALSA input on Linux and
  WinMM input on Windows; WinRT is an optional Windows backend.
- Keep the backend behind a trait/state machine so enumeration and reconnect
  can be tested with a fake backend.
- Verify output-port and event behavior in both OS-specific builds; never
  infer MK3 compatibility from a matching product name.

References:
- https://docs.rs/midir/latest/midir/
- https://github.com/Boddlnagg/midir

## Audio

Candidate approaches to prototype side-by-side:

1. rodio 0.23.x + cpal 0.18.x + symphonia 0.6.x for shared decode/mix and
   native device output. CPAL documents WASAPI on Windows and ALSA, PipeWire
   and PulseAudio backend options on Linux. Confirm that the chosen Linux host
   enumerates the same named PipeWire sinks that v2.4.0 routes through paplay.
2. Linux libpulse-binding direct streams for exact PipeWire-Pulse sink/source
   targeting, with a Windows WASAPI adapter through CPAL or wasapi. This is
   closer to released Linux semantics but adds a second implementation.
3. Direct PipeWire Rust bindings if Pulse compatibility lacks an endpoint or
   latency requirement needed by the app.

Audio must prove concurrent clips, per-clip sink, default fallback, second
sink mirror, volume 0-150%, loop, restart/toggle, stop during decode and
stream-open, stream/device removal, and VU capture. Do not choose based on
crate feature claims alone.

References:
- https://github.com/RustAudio/cpal
- https://docs.rs/rodio/latest/rodio/
- https://docs.rs/symphonia/latest/symphonia/
- https://docs.rs/libpulse-binding/latest/libpulse_binding/
- https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/

## Tray and window integration

- tray-icon 0.26.x supports Windows and Linux AppIndicator/KSNI.
- Linux backend and event-loop constraints differ between AppIndicator and
  KSNI; validate on KDE Plasma before enabling by default.
- Keep Esc-hide and tray behavior graceful when the desktop has no supported
  tray. Do not add global hotkeys or autostart without a real feature need.

Reference:
- https://docs.rs/tray-icon/latest/tray_icon/

## Cross-process and filesystem

- Use a dedicated local-only transport: Unix domain sockets on Linux and a
  per-user named pipe on Windows. Keep messages framed, bounded and
  deadline-limited; never add a TCP listener for local control.
- Unix protections include a 0700 directory, 0600 files, no-follow opens,
  ownership/link-count checks, same-directory temp plus atomic rename and
  fsync. Windows must use a current-user ACL and reject reparse-point paths;
  verify this with Windows integration tests rather than assuming Unix flags
  map across.
- User script execution is an explicit trusted configuration feature. Run it
  off callbacks, track process trees/job objects and make stop/quit
  generation-safe. Crosshair toggle is always direct argv and never shell.

## Release and build

Use a Cargo workspace and committed lockfile, cargo fmt, cargo clippy with
warnings denied, unit/integration tests, cargo-deny for license/advisory/source
review, CodeQL, reproducible build metadata, SHA-256 artifacts and a Windows/
Linux CI matrix. Linux packaging must document runtime PipeWire/ALSA
dependencies. Windows packaging must test per-user install, upgrade,
uninstall and rollback.
