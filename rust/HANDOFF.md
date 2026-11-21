# Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: Roaring1/Padfire
- Base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf`
- Branch: `rust/rewrite-foundation-20261120`
- Commit: `5743fa5bf353441292b1e98e47a5a3bd64ebdabb`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- The production v2.4.0 app remains the baseline. Do not stop/restart it or claim the physical Launchpad during development.

## Current implementation

The Rust workspace now contains the shared core, platform adapters and eframe desktop app. Config v5 migration, action/generation semantics, MIDI protocol and supervisor, audio playback/routing, script process supervision, Linux Unix-socket IPC, and Windows named-pipe IPC have implementations. Linux IPC CLI smoke verified status/show/reload/stop/quit, orderly exit and socket cleanup.

## Verification snapshot

- Nobara system compiler: workspace tests pass (67 tests).
- Nobara system compiler: `cargo check --workspace --all-targets --locked` passes.
- Nobara system compiler: optimized `cargo build --workspace --release --locked` passes.
- Stable rustfmt 1.100 toolchain installed locally; `cargo +stable fmt --all -- --check` passes after formatting.
- GitHub Actions run 1 initially failed on formatting before the formatting commit. Re-run CI after the next push.
- Windows CI has not yet passed; the initial Windows job also stopped at the formatting gate before compiling the Windows-only named-pipe source.
- Physical Launchpad, PipeWire/WASAPI, tray and Windows installer acceptance remain outstanding.

## Immediate next steps

1. Push the rustfmt-only corrections and confirm Ubuntu + Windows CI reaches tests, Clippy and release build.
2. Fix any Windows-only compiler/lint failures surfaced by that matrix.
3. Add Windows integration tests for named-pipe ownership, framing, timeout and orderly shutdown.
4. Complete tray/window actions and a low-overhead output meter.
5. Test storage/migration against real v2.4.0 configs in isolated temp profiles on both OSs.
6. Perform scheduled physical MK1/MK2 and audio routing acceptance without disturbing the live production app.
7. Complete packaging, security/dependency gates and rollback rehearsal before any cutover.

## Explicit non-goals until the gates pass

Do not replace v2.4.0, modify the production launcher, enable autostart, change live audio routing, or stop/restart the live Launchpad connection. Rust is a candidate, not yet a production cutover.
