# Padfire current handoff

Updated: 2026-11-24

## Current source of truth

- Repository: Roaring1/Padfire
- Default branch: main
- Current application: Padfire 3.0.0 stable, native Rust only
- Rust-only cleanup commit: `7652a22ffc64f733ca87539c5bd1e39bb5f8ef65`
- Only active branch: `main`; retired v2.4.0 release tag and old development branches removed. Archive checkpoint tags are non-installable history.
- The former GTK/Python runtime, launcher, packaging, Python tests and GTK-only workflow are retired from the active tree.
- GitHub before this cleanup: zero open pull requests, zero open issues, zero open main CodeQL alerts.

## Nobara deployment

- Installed binary: /home/roaring/.local/opt/padfire-rust/padfire-rust
- SHA-256 at ee5bd484: ade07c96739447ac52209804b69e94e0fbe7a762f522dd37c56c820f66bf3cc0
- Third-party notice: /home/roaring/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html
- User unit: padfire-rust.service, enabled for graphical-session.target
- Desktop identity: Padfire, icon padfire
- Config: /home/roaring/.config/padfire-rust/padfire.json, v5
- The separate LPD8 aseqdump monitor remains untouched.

## Verification

Local Linux: 144 Rust workspace tests, formatting, strict Clippy, optimized release build, cargo-deny advisories/licenses/bans/sources, offline cargo-about notice regeneration and byte comparison, and isolated Xvfb UI rendering passed. Hosted Ubuntu and Windows format/test/Clippy/release builds passed. Hosted dependency/license policy and Linux UI rendering passed. The hardware-free Rust smoke workflow passed. CodeQL default setup has been retired because it continued to schedule an empty Python scan. The advanced CodeQL workflow in `.github/workflows/codeql.yml` analyzes only Rust and Actions with the extended security query suite; confirm its fresh run after this cleanup.

The live process reported Launchpad Mini connected, page 0, no pad playing, and monitor enabled to pulseaudio:vm_music. CLI --status, --show and --reload were exercised.

## Stable 3.0.0 release scope and remaining validation

Padfire 3.0.0 is published as a stable source release with an unsigned Linux x86_64 binary. The Nobara KDE Wayland deployment is live with a connected Launchpad Mini and PipeWire/PulseAudio monitor. Hosted Windows format/test/Clippy/release-build CI is green, but Windows WASAPI and visible UI/tray have not had interactive acceptance. Signed release provenance, Flatpak and distro packages are not provided. Do not infer those items from CI or the Linux deployment.
