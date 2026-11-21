# Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: Roaring1/Padfire
- Base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf`
- Branch: `rust/rewrite-foundation-20261120`
- Implementation checkpoint: `e386bdbcca1b239632bc14e313a7a148cc07309f`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- Production v2.4.0 remains the reference. Do not stop/restart it or claim the physical Launchpad during development.

## Current implementation

The Rust workspace has a shared core, platform adapters and eframe desktop app. It includes v5 config compatibility/migration, action/generation semantics, MIDI protocol and reconnect supervisor, audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, native tray controls, cancellable FFmpeg LUFS analysis, and a low-overhead Padfire playback RMS meter.

- Tray menu: Show, Stop Audio, Quit; code-generated icon; initialized on the UI thread.
- LUFS analyzer: FFmpeg loudnorm on the first 120 seconds, -18 LUFS target, true-peak report, 1-150% suggested gain, explicit Apply action, cancellation, 30-second timeout and process reaping.
- Playback meter: 256-sample local accumulation, one packed AtomicU64 update per chunk, accounts for pad volume, aggregates concurrent Padfire sources, zero when idle, 35 ms attack / 240 ms release smoothing. It measures Padfire's mix, not unrelated system audio or post-device loopback.
- Windows IPC: local named pipe, first-instance ownership, inherited user-token DACL, remote-client rejection, bounded newline framing and response ACK to avoid dropping unread bytes on disconnect. Tests cover round-trip, shutdown/name release, second-instance rejection and unknown commands.
- Windows config storage: stable GetFileInformationByHandle hardlink check, with a Windows NTFS hardlink rejection test.

## Verification snapshot

- Nobara stable Rust 1.100: `cargo +stable fmt --all -- --check` passes.
- Nobara: `cargo +stable test --workspace --locked` passes (78 tests).
- Nobara: `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` passes.
- Nobara: `cargo +stable build --workspace --release --locked` passes.
- Real FFmpeg loudnorm integration test passes using a generated sine WAV.
- Isolated Xvfb + private D-Bus GUI smoke passes: tray registers; `--status`, `--show`, `--reload`, `--stop`, `--quit` respond; app exits cleanly and removes its socket. MIDI hardware was explicitly disabled during this smoke.
- GitHub Actions matrix on `e386bdb` passes on Ubuntu and Windows: formatting, tests, strict Clippy and optimized release build.
- Actual Windows laptop: Rust 1.100 and minimal Visual C++ Build Tools installed. Native `cargo fmt --all -- --check`, all 27 Windows tests, strict Clippy and optimized release build all pass. Windows tests include the named-pipe and hardlink regressions.
- Windows laptop checkout: `%USERPROFILE%\src\Padfire`; use the VS Build Tools `vcvars64.bat` environment before MSVC Cargo commands.

## Immediate next steps

1. Implement side-by-side install/uninstall and release packaging. Do not overwrite or remove the Python v2.4.0 production install; Rust must install as a separate candidate, and uninstall must preserve user config.
2. Complete config migration fixtures using real v2.4.0 settings on both OSs; add Windows ACL/reparse-point and crash/power-loss durability tests.
3. Perform physical MK1/MK2 input/LED/unplug/reconnect acceptance and PipeWire/WASAPI routing acceptance without disturbing the production app.
4. Verify the playback meter against real audio on both platforms; decide whether Linux needs an optional sink-loopback mode to match the legacy system-output meter.
5. Complete window/focus/close behavior, packaging/update/rollback rehearsal, dependency/license/security gates and installer/uninstaller acceptance.

## Explicit non-goals until gates pass

Do not replace v2.4.0, modify the production launcher, enable autostart, change live audio routing, or stop/restart the live Launchpad connection. Rust is a candidate, not yet a production cutover.
