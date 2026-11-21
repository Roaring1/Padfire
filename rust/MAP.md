# Rust migration map

Status key: **implemented** = code exists; **verified** = automated tests or smoke evidence pass; **manual** = requires physical/device acceptance; **not started** = no implementation yet.

| Area | Current state | Implementation / evidence | Remaining gate |
|---|---|---|---|
| v5 configuration model and migration | verified | `padfire-core::config`; compatibility fixtures cover v1-v5, unknown-field preservation, malformed normalization and released defaults | Compare additional real-world v2.4.0 config fixtures on both OSs |
| Secure configuration storage | implemented, Linux-tested | `padfire-platform::config_store`; atomic replacement, backups, size limits, symlink/hardlink checks and Unix ownership/mode checks | Windows ACL/reparse-point review and native tests; crash/power-loss test |
| App state and pad action dispatch | verified | `padfire-core::actions` and `runtime`; generation-safe stop/retrigger and per-pad routing tests | Full legacy behavior matrix comparison |
| CLI and single-instance IPC | Linux verified; Windows implemented, unverified | fixed command protocol, bounded framing, Unix owner-only socket, Windows named pipe with first-instance and remote-client rejection | Windows CI compile/test; compare CLI JSON and command behavior with v2.4.0 |
| MIDI protocol and Launchpad Mini profile | decoder verified; backend implemented | `padfire-core::midi`, `padfire-platform::midi`; MK3 excluded, pair selection avoids guessing, reconnect supervisor and LED protocol | Physical MK1/MK2 input, LED, unplug/replug and reconnect on Nobara and Windows |
| Audio decoding, playback and routing | implemented; automated logic tested | `padfire-platform::audio_engine`; decode/play/loop/retrigger, per-pad/global output, monitor stream and cancellation | Physical PipeWire and WASAPI tests, simultaneous playback, device removal and audio-server restart |
| VU / output meter | not started | No meter/capture module exists yet | Design low-overhead per-output RMS/peak capture with explicit device lifecycle |
| Script process supervision | implemented; Linux-tested | `padfire-platform::process_manager`; bounded output, generation cancellation, process-group/tree termination and Crosshair resolver | Windows process-tree tests; verify no orphan descendants on both OSs |
| Loudness analyzer | not started | No analyzer module or reference fixtures yet | Decide whether this remains in v3 scope; if retained, compare against FFmpeg reference output |
| Native UI shell | implemented; basic Linux smoke | eframe/egui shell in `apps/padfire/src/main.rs`; pad grid, page selection, editor and settings surfaces | Visual/keyboard/focus/resize acceptance on KDE Wayland and Windows 11 |
| Tray and window integration | implemented; Linux smoke verified | App-thread tray initialization with Show, Stop Audio and Quit menu; code-generated icon; private D-Bus/Xvfb smoke passes | Windows native menu smoke; KDE Wayland and Windows 11 visual/focus acceptance; decide close-to-tray behavior |
| Install / update / uninstall | not started | `--install` explicitly reports unavailable | Side-by-side install, shortcuts, clean uninstall, no autostart, rollback and signed Windows packaging |
| CI | implemented; fresh matrix pending | `.github/workflows/rust.yml` runs Ubuntu and Windows stable toolchains, fmt, tests, Clippy and release builds | Verify latest pushed commit passes both OS jobs, including Windows-only IPC and tray compilation |
| Security / supply-chain gates | not started | Locked dependencies and bounded input handling exist | cargo-deny/license audit, dependency review, CodeQL/SAST and release provenance |
| Dual-machine acceptance / release | not started | Cutover and rollback checklist in `CUTOVER.md` | Real-device acceptance on Nobara and Windows, artifacts/checksums and rollback rehearsal |

## Dependency order

1. Stabilize shared domain/config behavior and keep the released v2.4.0 app as the reference.
2. Complete platform storage, IPC and process lifecycle on both OSs.
3. Verify MIDI discovery/reconnect and hardware protocol on actual Mini MK1/MK2 devices.
4. Verify audio decoding, mixing, routing, monitor and stop races on PipeWire and WASAPI.
5. Complete the tray/window experience, meter and remaining UI parity.
6. Add installer/updater/uninstaller, security gates and signed release artifacts.
7. Perform dual-machine acceptance and only then cut over the default launcher.

Parallel work is safe only where the shared core contracts are stable. Do not let UI, audio and MIDI invent independent config or action models.
