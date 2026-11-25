# Deployment and remaining production gates

## Current state - 2026-11-24

The native Rust application is the only installed Padfire runtime on the Nobara workstation and the only active implementation in the default branch. The old GTK/Python v2.4.0 runtime, startup files, user configs, desktop identity and local binary rollback copies are removed. The old active Python source/packaging/tests are removed from main as well. The v2.4.0 tag is retired; Git commit history and the dated changelog remain archival records, not active installable app entries.

- Version: Padfire 3.0.0 stable
- Security-remediation base before Rust-only retirement: ee5bd48486b5e3775edd21c4a5acd1009b63d150
- Rust-only cleanup commit: 7652a22ffc64f733ca87539c5bd1e39bb5f8ef65
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
- CodeQL default setup is disabled; `.github/workflows/codeql.yml` now analyzes Rust and Actions only (the retired Python source is no longer scanned).

## Stable 3.0.0 release scope and remaining acceptance

The stable release contains the source and an unsigned Linux x86_64 binary. The active Nobara KDE Wayland deployment was verified with the Launchpad Mini connected and the PipeWire/PulseAudio monitor active. Automated Linux and Windows build/test/Clippy, Linux UI renders, dependency/license validation and CodeQL passed.

Remaining work is explicitly outside this release: full documented physical pad/LED/reconnect and playback/VU acceptance, interactive Windows WASAPI/UI/tray acceptance, signed provenance, Flatpak and distro packages. Windows CI passing is not a substitute for interactive Windows validation. The current stable binary remains installed and live.
