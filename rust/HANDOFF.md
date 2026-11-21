# Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: Roaring1/Padfire
- Base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf`
- Branch: `rust/rewrite-foundation-20261120`
- Latest commit: `f2e9e3651e0c7b044c6f2d13646d547de63785ef`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- Production v2.4.0 remains the reference. Do not stop/restart it or claim the physical Launchpad during development.

## Current implementation

The Rust workspace has a shared core, platform adapters and eframe desktop app. It includes v5 config compatibility/migration, action/generation semantics, MIDI protocol and reconnect supervisor, audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, native tray controls, cancellable FFmpeg LUFS analysis, and a lock-free Padfire playback RMS meter.

- Tray menu: Show, Stop Audio, Quit; code-generated icon; initialized on the UI thread.
- LUFS analyzer: FFmpeg loudnorm on the first 120 seconds, -18 LUFS target, true-peak report, 1-150% suggested gain, explicit Apply action, cancellation, 30-second timeout and process reaping.
- Playback meter: 256-sample local accumulation, one packed AtomicU64 update per chunk, accounts for pad volume, aggregates concurrent Padfire sources, zero when idle. It is not a system-wide or post-device loopback meter.
- Windows IPC tests cover round-trip, first-instance ownership, unknown command rejection and shutdown/name release.

## Verification snapshot

- Stable Rust 1.100: `cargo +stable fmt --all -- --check` passes.
- Stable Rust: `cargo +stable test --workspace --locked` passes (76 tests: 28 core, 17 process/runtime, 31 platform).
- Stable Rust: `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` passes on Nobara.
- Stable Rust: `cargo +stable build --workspace --release --locked` passes.
- Real FFmpeg loudnorm integration test passes on Nobara using a generated sine WAV.
- Isolated Xvfb + private D-Bus GUI smoke passes: tray registers; `--status`, `--show`, `--reload`, `--stop`, `--quit` respond; app exits cleanly and removes its socket. MIDI hardware was explicitly disabled during this smoke.
- Latest GitHub Actions matrix for `f2e9e36` is queued/running on Ubuntu and Windows. Previous CI proved Windows tests compile/pass, but Windows Clippy failed on scoped unsafe-code linting and a needless-bool warning; both have been corrected and need confirmation on this latest run.
- Ubuntu CI previously passed tests and Clippy after adding ALSA development headers; its release build was still running at last inspection.
- Windows laptop SSH works, but Cargo/Rust are not installed there. Native Windows CI is the current compile gate.

## Immediate next steps

1. Inspect both CI jobs for `f2e9e36`; fix any remaining Windows Clippy or release-build findings.
2. Verify the new Windows named-pipe integration tests actually execute and pass on the Windows runner.
3. Add/verify Windows config ACL and reparse-point tests, plus isolated real v2.4.0 config migration fixtures on both OSs.
4. Perform physical MK1/MK2 input/LED/unplug/reconnect and PipeWire/WASAPI routing acceptance without disturbing the production app.
5. Verify the playback meter against real audio on both platforms and decide whether Linux needs an optional sink-loopback mode to match the legacy system-output meter.
6. Complete close/window/focus visual acceptance, installers/updater/uninstaller, dependency/license/security gates and rollback rehearsal.

## Explicit non-goals until gates pass

Do not replace v2.4.0, modify the production launcher, enable autostart, change live audio routing, or stop/restart the live Launchpad connection. Rust is a candidate, not yet a production cutover.
