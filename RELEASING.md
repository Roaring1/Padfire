# Release process

The active application is the native Rust workspace in rust/. The old GTK/Python application has been retired from the default branch and workstation installation. Do not publish a stable tag until the alpha's remaining physical-device, Windows interactive UI/audio and signing/provenance gates are recorded.

## Before a release

1. Update the workspace version in rust/Cargo.toml and add a dated section to CHANGELOG.md. Keep Cargo.lock and the generated third-party license notice consistent.
2. Run the full local gates in CONTRIBUTING.md.
3. Review the complete diff for configuration migration, filesystem boundaries, IPC, process execution, audio lifecycle, dependency advisories, accepted licenses and provenance.
4. Confirm the exact candidate commit has successful Ubuntu and Windows Rust workflows, Linux UI rendering, hardware-free smoke tests, cargo-deny/license report validation and Rust and Actions CodeQL results. Review the open Code Scanning inventory separately.
5. Complete physical Launchpad Mini MK1/MK2 input, page, pad, LED and reconnect tests. Verify real playback, stop, looping/retrigger, routing, monitor and VU behavior on Nobara/PipeWire and Windows/WASAPI.
6. Review the installed binary and third-party notice hashes against the exact build artifacts. Record signing and provenance evidence.
7. Merge the candidate to main through a reviewed pull request.

## Publish

Only after all gates pass:

    git switch main
    git pull --ff-only
    git status --short
    cargo build --workspace --release --locked
    git tag -a vX.Y.Z -m "Padfire vX.Y.Z"
    git push origin vX.Y.Z
    gh release create vX.Y.Z --verify-tag --title "Padfire vX.Y.Z" --notes-file RELEASE_NOTES.md

Use the actual version and release-note path. Do not tag a dirty worktree, an unmerged branch, or a build whose binary/notice hashes do not match the validated artifacts. Do not label the current alpha as a stable release.

## Distribution scope

The Rust installer supports per-user Linux and Windows installation. A source checkout or green CI run is not a signed distribution. Flatpak, distro packages, automatic updates and release signing/provenance must be completed and reviewed before they are advertised.
