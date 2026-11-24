# Padfire current handoff

Updated: 2026-11-24

## Current source of truth

- Repository: Roaring1/Padfire
- Default branch: main
- Current application: Padfire 3.0.0-alpha.1, native Rust only
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

Local Linux: 144 Rust workspace tests, formatting, strict Clippy, optimized release build, cargo-deny advisories/licenses/bans/sources, offline cargo-about notice regeneration and byte comparison, and isolated Xvfb UI rendering passed. Hosted Ubuntu and Windows format/test/Clippy/release builds passed. Hosted dependency/license policy and Linux UI rendering passed. The hardware-free Rust smoke workflow passed. CodeQL Rust/Python/Actions passed on the then-current main head; open CodeQL alert count on main was zero.

The live process reported Launchpad Mini connected, page 0, no pad playing, and monitor enabled to pulseaudio:vm_music. CLI --status, --show and --reload were exercised.

## Remaining gates

Physical Launchpad MK1/MK2 pad/LED/reconnect; real PipeWire playback/monitor/VU; Windows WASAPI and visible UI/tray; signed release/provenance. Do not claim these manual checks passed based on CI. The application remains alpha until recorded.
