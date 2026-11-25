# Release process

## Padfire 3.0.0 stable release

The maintainer has authorized the first stable version as a scoped source release with an unsigned Linux x86_64 binary. The release is tested and deployed on Nobara KDE Wayland with a connected Launchpad Mini and PipeWire/PulseAudio monitor. The automated Linux/Windows build matrix, tests, Clippy, Linux UI render, dependency/license checks and Rust/Actions CodeQL are required before the stable tag is published.

The following limitations are part of the release scope and must not be represented as complete:

- Windows builds and automated tests pass, but Windows WASAPI, visible UI and tray behavior have not received interactive acceptance.
- The documented full physical Launchpad pad/LED/reconnect and real playback/monitor/VU acceptance matrix is not claimed complete by CI or connection status alone.
- The Linux binary is unsigned. No signed provenance attestation, Flatpak, distro package or automatic updater is produced.

## Before a release

1. Confirm `main` is clean, pushed, and contains no unfinished pull requests or issues.
2. Update the workspace version, changelog, release notes, lockfile and generated third-party license notice together.
3. Run formatting, the complete workspace tests, strict Clippy, optimized builds and cargo-deny.
4. Confirm the exact candidate commit has successful Ubuntu and Windows Rust workflows, Linux UI rendering, cargo-deny/license verification, Rust and Actions CodeQL results, and review the open Code Scanning inventory.
5. Record real-device and platform acceptance accurately. Do not infer pad presses, LED output, audio playback, reconnect, Windows UI or signing from compile/test CI.
6. Verify installed binary and third-party notice hashes against the release artifacts.

## Publish

For the approved stable Linux/source release:

    git switch main
    git pull --ff-only
    git status --short
    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --release --locked
    cargo deny check
    git tag -a vX.Y.Z -m "Padfire vX.Y.Z"
    git push origin vX.Y.Z
    gh release create vX.Y.Z --verify-tag --title "Padfire vX.Y.Z" --notes-file RELEASE_NOTES.md <assets>

Use the actual version and release-note path. Do not tag a dirty worktree, an unpushed commit, a prerelease version, or a build whose binary/notice hashes do not match the validated artifacts.

## Distribution scope

The Rust installer supports per-user Linux and Windows installation. A source checkout or green CI run is not a signed distribution. Flatpak, distro packages, automatic updates and release signing/provenance remain separate work and must not be advertised until implemented and reviewed.

Future stable releases must complete the relevant physical-device and Windows interactive acceptance gates or explicitly document a maintainer-approved, narrower release scope as done for 3.0.0.
