# Contributing

Padfire's current implementation is the native Rust workspace in rust/. Do not add a second GTK, Qt, Python or webview frontend. Keep Linux and Windows behavior behind the padfire-platform adapters and shared product behavior in padfire-core.

## Required local checks

From rust/:

    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --release --locked
    cargo deny check

When dependencies or features change, regenerate the third-party notice with the pinned cargo-about 0.9.3 tool and compare it byte-for-byte with THIRD_PARTY_LICENSES.html. Run git diff --check from the repository root.

For Linux UI changes, use the isolated Xvfb renderer in rust/review/render-ui.sh. It runs without physical hardware or the user's real configuration. Never open a physical MIDI port or play audio from an unattended automated test.

## Manual acceptance

Before a stable release, record actual results for Launchpad Mini MK1/MK2 pad input, page changes, LED output, USB unplug/replug, real PipeWire playback, stop/loop/retrigger, output routing and monitor/VU behavior. Repeat the audio/UI acceptance on Windows with WASAPI and a visible desktop session. Hosted CI does not substitute for physical-device acceptance.

## Configuration and privacy

The current v5 configuration is ~/.config/padfire-rust/padfire.json. Pad assignments can contain local file paths and executable shell commands. Never attach a live padfire.json or screenshots containing private paths to an issue without review and redaction. Treat imported Script-pad assignments as executable code.
