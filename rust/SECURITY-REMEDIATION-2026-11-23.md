# Padfire Rust security remediation - 2026-11-23

## Status and boundaries

This is an isolated security-remediation branch based on main. It is not a release and it does not change the installed Padfire v2.4.0 GTK application, its config, audio routing, service state, or Launchpad integration. Keep v2.4.0 as the production fallback until the Rust candidate completes its separate hardware and install/uninstall acceptance.

The repository's CodeQL dashboard was inspected before these changes. It showed 104 open Rust alerts:

- 100 rust/path-injection alerts (high).
- 3 rust/command-line-injection alerts (critical).
- 1 rust/unsafe-deserialization alert (critical).

Those are baseline counts, not a claim about a fresh scan of this branch. No alerts have been dismissed as part of this work. A fresh CodeQL result on the proposed PR is required before deciding which findings have actually cleared.

## Changes made

### Filesystem paths and uninstall safety

- Uninstall no longer recursively deletes the configured install root. It removes only Padfire's known executable, desktop entry/shortcut and icon, then removes the install directory only if it is empty.
- The Windows delayed-uninstall script now deletes only padfire-rust.exe and attempts a non-recursive empty-directory removal. Other contents are retained.
- A directory or symlink where a Padfire file is expected is not recursively removed.
- PADFIRE_INSTALL_ROOT must be absolute and cannot contain parent-directory traversal.
- HOME, XDG_DATA_HOME, APPDATA, LOCALAPPDATA, and the optional Start Menu override are validated as absolute paths without parent traversal. Existing ancestors are canonicalized on Unix; Windows retains native Win32 spelling while rejecting symlink/reparse-point ancestors.
- Installer writes, rollback, install and uninstall preflight directory ancestors and reject symlinks/reparse points and non-directory components.
- Windows PowerShell helper scripts and delayed-uninstall diagnostics are created under the native system temp directory using unique create_new files, then flushed and synced. The ordinary Win32 path spelling is preserved for PowerShell/COM consumers; existing temp-directory ancestors are checked for reparse points. The uninstall diagnostic no longer uses a predictable sibling path beside the install directory, so it cannot overwrite an unrelated uninstall.log file.
- Test fixtures use the OS-provided temp directory spelling. Canonicalizing every Windows temp path can add a \\?\ prefix that cmd.exe and some Shell integrations do not accept, so canonicalization is limited to code paths whose OS APIs support it.

### Config storage and deserialization

- Config base directories must be absolute and cannot contain parent-directory traversal; existing paths are normalized to canonical paths.
- Config reads open the final filename below a canonical parent directory. The Unix path continues to use no-follow/non-blocking open flags, rejects hardlinks and foreign ownership, and applies private file permissions. Windows retains reparse-point, hardlink and ACL checks.
- JSON input remains capped at 4 MiB.
- The complete parsed JSON tree is checked iteratively with a maximum depth of 64 and a maximum of 100,000 nodes, including unknown fields retained for forward compatibility.
- The normalized config permits at most 256 pages and 128 pad entries per page. Known path, command, label, notes and action strings have explicit byte limits and reject NUL characters.
- Compatibility tests verify that valid extra pages, custom volume values and forward-compatible unknown fields still round-trip.

The typed conversion still uses serde_json::from_value after these explicit bounds and normalization steps. The baseline unsafe-deserialization alert must be re-evaluated against a fresh CodeQL scan and human review; it is not marked resolved in this document.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
- PID status-file reads reject non-regular files, oversize content, symlinks/reparse points, foreign-owned Linux files and hardlinks.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- Crosshair lookup / baseline command-line-injection alert #16: the app intentionally discovers the separately installed Crosshair CLI from PATH or the user-local install path and runs it with a fixed --toggle argument. The selected executable is canonicalized and checked as a regular executable, but the selected binary remains controlled by the user's environment. This is an execution trust boundary, not a shell command built from the config. Do not claim the CodeQL alert is resolved until a fresh scan and review confirm the behavior.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use; the static alert still needs fresh-scan review.

## Validation performed on this branch

Latest completed local Linux validation before this document was added:

- cargo fmt --all -- --check
- cargo test --workspace --locked
- cargo clippy --workspace --all-targets --locked -- -D warnings
- cargo build --workspace --release --locked

All passed. The last run covered 126 tests: 18 app unit tests, 2 CLI integration tests, 28 core unit tests, 21 compatibility tests and 57 platform tests. It ran on Nobara/Linux. Windows compilation and tests still require the hosted Windows CI matrix; Linux checks do not substitute for them.

## Remaining acceptance

1. Push this branch and create a draft PR so the Linux and Windows Rust CI and CodeQL can evaluate the exact proposed commit.
2. Review the new CodeQL alert set by source-to-sink trace. Do not bulk-dismiss test-path or intended-feature findings without confirming the full flow.
3. Review any remaining deserialization and PATH-selected Crosshair findings against the bounds/trust model above.
4. Complete physical Launchpad MK1/MK2, KDE Wayland/PipeWire loopback, Windows WASAPI, UI/tray/input, side-by-side install/uninstall, config migration and rollback acceptance before production cutover.
5. Keep v2.4.0 as production until those gates pass.
