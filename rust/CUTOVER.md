# Deployment and remaining production gates

## Current state - 2026-11-24

The native Rust application is the only installed Padfire runtime on the Nobara workstation and the only active implementation in the default branch. The old GTK/Python v2.4.0 runtime, startup files, user configs, desktop identity and local binary rollback copies are removed. The old active Python source/packaging/tests are removed from main as well. The v2.4.0 tag is retired; Git commit history and the dated changelog remain archival records, not active installable app entries.

- Version: Padfire 3.0.0-alpha.1
- Main source commit before this cleanup: ee5bd48486b5e3775edd21c4a5acd1009b63d150
- Installed executable: ~/.local/opt/padfire-rust/padfire-rust
- Active config: ~/.config/padfire-rust/padfire.json (v5)
- User service: padfire-rust.service, enabled for graphical-session.target
- Desktop entry/icon identity: Padfire / padfire.svg
- Third-party license notice: ~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html

There is no local v2.4.0 executable or older Rust binary backup. Rebuild a prior commit from Git history only if recovery is explicitly needed; do not leave duplicate runtimes installed.

## Service management

    systemctl --user status padfire-rust.service
    systemctl --user start padfire-rust.service
    systemctl --user stop padfire-rust.service
    systemctl --user restart padfire-rust.service

The service starts with the user's graphical session and runs as the user. It is not a system/root service. It is the native desktop application, not a hidden audio daemon.

## Automated verification completed

- Local Linux: 144 Rust workspace tests, formatting, strict Clippy with warnings denied, optimized release build, cargo-deny and cargo-about byte comparison passed.
- Hosted Linux/Windows: format, tests, strict Clippy and optimized release builds passed.
- Headless UI: main grid and Settings renders passed under Xvfb; hosted Linux UI render passed.
- GTK/Python legacy smoke workflow was retired with that application; the Rust UI and Rust hardware-free tests are the active checks.
- CodeQL Rust, Python and Actions analyses on ee5bd484 completed; main-branch open alert inventory was zero.

## Remaining stable-release acceptance

1. Physically test Launchpad Mini MK1/MK2 pad presses, page controls, LED output and USB disconnect/reconnect.
2. Verify real PipeWire playback, per-pad routing, stop/loop/retrigger, monitor loopback and VU response.
3. Verify Windows WASAPI playback/routing and visible Windows UI/tray behavior.
4. Complete signed release artifacts and provenance checks before creating a stable release/tag.

The alpha remains installed and live while those manual gates are pending.
