# Padfire

Native Rust soundboard and Launchpad controller for Linux and Windows. The active application is Padfire 3.0.0-alpha.1. The old GTK/Python v2.4.0 runtime and its packaging have been retired; the current source tree and workstation install use the Rust application only.

## Features

- Eight pages of 8x8 pads with top-row page switching and side-column assignments.
- Per-pad sound playback or a user-configured script action, with per-pad volume and output routing.
- Loop/retrigger, stop targets, labels, colors and notes.
- Launchpad Mini MK1/MK2 MIDI input/output with disconnect detection and reconnect. MK3 and unknown controllers are not sent speculative output messages.
- Native desktop UI, editor, settings, tray controls and keyboard shortcuts.
- Local, same-user status/control IPC; no network listener.
- PipeWire/PulseAudio playback and monitor on Linux; WASAPI on Windows.
- Optional FFmpeg LUFS analysis and playback/output metering.

## Build

The Rust workspace is in rust/. From that directory run:

    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --release --locked
    cargo deny check

GitHub Actions runs the Linux and Windows Rust matrix, the hardware-free test suite, dependency/license verification, CodeQL, and a headless rendering of the real Linux UI. See rust/README.md for the headless render command.

## Installation and control

The per-user installer creates one Padfire application entry and installs the third-party license notice. On Nobara the installed executable is ~/.local/opt/padfire-rust/padfire-rust. The active v5 user configuration is ~/.config/padfire-rust/padfire.json.

The current Nobara deployment is managed by the padfire-rust.service user unit:

    systemctl --user status padfire-rust.service
    systemctl --user start padfire-rust.service
    systemctl --user stop padfire-rust.service

The executable also accepts --version, --status, --show, --stop, --reload, --quit, --install and --uninstall.

## Security

Padfire is a local, single-user desktop application. The Script action runs the command explicitly configured by the user with that user's permissions. Treat imported pad configurations as code. IPC is restricted to the current user and is not exposed over the network. See SECURITY.md for the security boundary and reporting guidance.

## Status

The full Rust automated validation is green on Linux and Windows. Physical Launchpad MK1/MK2 pad/LED/reconnect acceptance, real PipeWire playback/loopback/VU verification, Windows WASAPI and visible UI/tray acceptance, and release signing/provenance remain manual release gates. This is an alpha build, not a stable release.

## Project records

- rust/README.md - Rust workspace, build, deployment and configuration
- rust/BEHAVIOR_MATRIX.md - behavior coverage and acceptance evidence
- rust/MAP.md - implementation/test-status map
- rust/HANDOFF.md - current project checkpoint
- CONTRIBUTING.md - development checks
- RELEASING.md - release requirements
- ROADMAP.md - deferred device-profile ideas
- CHANGELOG.md - historical release notes
