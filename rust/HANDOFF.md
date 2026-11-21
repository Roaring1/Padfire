# Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: Roaring1/Padfire
- Base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf`
- Branch: `rust/rewrite-foundation-20261120`
- Latest commit: `f4a8646275c81877faec979ccc0a96c2b3904e64`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- Production v2.4.0 remains the reference. Do not stop/restart it or claim the physical Launchpad during development.

## Current implementation

The Rust workspace has a shared core, platform adapters and eframe desktop app. It includes v5 config compatibility/migration, action/generation semantics, MIDI protocol and reconnect supervisor, audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, and a native tray menu (Show, Stop Audio, Quit). The tray icon is generated in code and the tray is initialized on the UI thread.

## Verification snapshot

- Stable Rust 1.100: `cargo +stable fmt --all -- --check` passes.
- Stable Rust: `cargo +stable test --workspace --locked` passes (67 tests).
- Stable Rust: `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` passes.
- Stable Rust: `cargo +stable build --workspace --release --locked` passes.
- Isolated Xvfb + private D-Bus GUI smoke: tray registration passes; `--status`, `--show`, `--reload`, `--stop`, `--quit` all respond; the app exits cleanly and removes the socket. MIDI hardware was explicitly disabled during this smoke.
- Linux IPC shutdown smoke passes.
- GitHub Actions run on the latest tray-enabled commit `f4a8646` is queued/running for Ubuntu and Windows. Inspect both jobs before further Windows-specific changes.
- Windows laptop SSH works, but Cargo/Rust are not installed there. Native Windows CI is the current compile gate.

## Immediate next steps

1. Inspect the fresh Ubuntu and Windows jobs for commit `f4a8646`; fix Windows-only compile/lint/test failures.
2. Add Windows named-pipe integration tests for ownership, framing, timeout and shutdown.
3. Add a low-overhead output meter; no meter/capture module exists yet.
4. Review Windows ACL/reparse-point behavior for config storage and test migration using isolated real v2.4.0 config fixtures on both OSs.
5. Perform physical MK1/MK2 input/LED/unplug/reconnect and PipeWire/WASAPI routing acceptance without disturbing the production app.
6. Complete close/window/focus visual acceptance, installers/updater/uninstaller, dependency/license/security gates and rollback rehearsal.

## Explicit non-goals until gates pass

Do not replace v2.4.0, modify the production launcher, enable autostart, change live audio routing, or stop/restart the live Launchpad connection. Rust is a candidate, not yet a production cutover.
