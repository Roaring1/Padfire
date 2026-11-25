# Padfire

Padfire is the native Rust soundboard and Launchpad controller. The active implementation is version 3.0.0-alpha.1. The former GTK/Python v2.4.0 application has been retired from the current source tree and workstation installation.

## Workspace

- crates/padfire-core: configuration schema and migration, action semantics, audio routing decisions, MIDI packet model and IPC contract.
- crates/padfire-platform: secure configuration storage, MIDI discovery/reconnect, audio playback, process supervision, per-user installation and local IPC.
- apps/padfire: native eframe/egui desktop app, pad editor, settings, tray, runtime and command-line interface.

The same Rust application is built for Linux and Windows. Linux uses the PulseAudio compatibility layer provided by PipeWire where available; Windows uses WASAPI. Platform-specific behavior remains behind the platform crate.

## Build and validate

Run from this directory:

    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --release --locked
    cargo deny check

The dependency workflow also regenerates the third-party notice with cargo-about 0.9.3 and verifies it byte-for-byte. Hosted CI runs the Rust matrix on Ubuntu and Windows and renders the actual UI in a headless Linux desktop.

For a local headless UI review on Linux, build first and run:

    out=$(mktemp -d /tmp/padfire-review.XXXXXX)
    xvfb-run -a bash review/render-ui.sh target/release/padfire "$out"

The renderer disables real hardware and personal configuration. It writes main-grid and Settings screenshots into the selected output directory.

## Install and run

A per-user install uses the native executable and creates the Padfire desktop entry plus the third-party license notice. On Linux the managed executable directory is ~/.local/opt/padfire-rust. The normal app identity is Padfire; Rust is the implementation.

Useful commands:

    padfire-rust --version
    padfire-rust --status
    padfire-rust --show
    padfire-rust --stop
    padfire-rust --reload
    padfire-rust --quit
    padfire-rust --install
    padfire-rust --uninstall

The Nobara workstation runs the installed binary through the padfire-rust.service user unit, attached to graphical-session.target. The unit starts with the graphical session and is managed with systemctl --user. The installer itself does not silently create or enable a service.

The main window is fixed-size and cannot be resized by dragging. Its min/max dimensions track the pad-grid footprint. Use Settings > UI scale to resize the entire interface and its window together from 75% to 125% in 5% increments; Save persists the choice, while Cancel restores the previous scale. The built-in keyboard zoom shortcuts are disabled so the window cannot drift away from the selected size.

On KDE Wayland, the titlebar close button sends Padfire to the tray instead of exiting. KWin hides the window from the taskbar, pager and app switcher while the soundboard process and controller workers stay active. Left-click the tray icon or choose Show from its menu to restore and focus the window. This uses a short-lived, process-scoped KWin script through `qdbus`, because Wayland does not support the generic window hide/unminimize calls used by other backends.

## User configuration

The active v5 configuration is ~/.config/padfire-rust/padfire.json. Saves are atomic and keep a rolling padfire.json.bak. The current Rust runtime config is the source of truth. A guarded one-time migration reader for the old ~/.config/padfire/padfire.json path remains for clean upgrades, but the legacy file has been removed from the deployed workstation after its assignments were imported.

The configuration supports eight pages, 64 pads per page, per-pad files/actions/output routing, volume, looping, labels, colors, notes, stop targets and retrigger behavior. Only Launchpad Mini MK1/MK2 output protocols are supported. Unknown devices and MK3 are not probed with speculative LED messages.

## Security and trust

Padfire is a local, single-user application, not a network service. Configuration and IPC are owner-restricted and bounded. User-authored Script pad actions intentionally execute the configured command with the user's permissions; treat pad configurations as executable content. Only import trusted configurations. Crosshair is resolved from the user-session PATH and launched with a fixed argument, not through a shell.

The checked-in third-party license notice is rust/THIRD_PARTY_LICENSES.html. It is installed beside the Linux executable and included in the Windows installation.

## Remaining stable-release acceptance

The automated matrix is green, but stable release acceptance still requires physical Launchpad pad/LED/reconnect tests, actual PipeWire playback and monitor/VU verification, Windows WASAPI and visible UI/tray acceptance, and signed release provenance. The current version remains alpha until those gates are recorded.
