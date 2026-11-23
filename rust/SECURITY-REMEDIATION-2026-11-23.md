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
- The install root is fixed to the OS-defined per-user application location (HOME/.local/opt on Linux, LOCALAPPDATA/Programs on Windows). The unused PADFIRE_INSTALL_ROOT override was removed so an environment value cannot redirect installation or uninstall to an arbitrary path.
- HOME, XDG_DATA_HOME, APPDATA, LOCALAPPDATA, and the optional Start Menu override are validated as absolute paths without parent traversal. The nearest existing ancestor is canonicalized before descendants are formed. Windows rejects symlink/reparse-point ancestors, then removes only the extended-length prefix from the physical path before passing it to PowerShell/COM.
- Installer writes, rollback, install and uninstall preflight directory ancestors and reject symlinks/reparse points and non-directory components.
- Windows PowerShell helper scripts and delayed-uninstall diagnostics are created under the native system temp directory using unique create_new files, then flushed and synced. PowerShell script names include an atomic per-process sequence as well as PID/time because Windows clock resolution can be coarser than nanoseconds. A failed create_new never removes the conflicting path; cleanup only occurs after this call successfully owns the file. This prevents parallel installer operations from deleting another in-flight script. The ordinary Win32 path spelling is preserved for PowerShell/COM consumers; existing temp-directory ancestors are checked for reparse points. The uninstall diagnostic no longer uses a predictable sibling path beside the install directory, so it cannot overwrite an unrelated uninstall.log file.
- Test fixtures use the OS-provided temp directory spelling. Canonicalizing every Windows temp path can add a \\?\ prefix that cmd.exe and some Shell integrations do not accept, so canonicalization is limited to code paths whose OS APIs support it.

### Config storage and deserialization

- Config base directories must be absolute and cannot contain parent-directory traversal; existing paths are normalized to canonical paths.
- Config reads open the final filename below a canonical parent directory. The Unix path continues to use no-follow/non-blocking open flags, rejects hardlinks and foreign ownership, and applies private file permissions. Windows retains reparse-point, hardlink and ACL checks.
- JSON input remains capped at 4 MiB.
- JSON is parsed by a bounded JSON tokenizer/parser that enforces maximum nesting depth 64 and 100,000 value nodes before allocating nested arrays, objects or strings. Object keys and unknown extension values remain supported; total input is capped at 4 MiB.
- The normalized config permits at most 256 pages and 128 pad entries per page. Known path, command, label, notes and action strings have explicit byte limits and reject NUL characters.
- Compatibility tests verify that valid extra pages, custom volume values and forward-compatible unknown fields still round-trip.

After bounded parsing and schema normalization, the typed AppConfig conversion uses Serde's Deserialize implementation over the already-bounded Value tree. The untrusted byte stream is not passed to serde_json::from_slice/from_reader, and the code does not call the CodeQL-modeled serde_json::from_value sink. The latest Rust CodeQL PR analysis on f6de3a6 reports one high-severity path-injection result and no unsafe-deserialization result; the baseline critical deserialization finding is therefore absent from the current PR analysis. Reconfirm on the next pushed commit.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
- The optional PID status-file path remains user-selectable because script authors may choose where their script writes the PID list. The resolver rejects parent traversal and canonicalizes the existing parent while preserving the final filename. It is opened directly with no-follow/nonblocking flags on Linux or OPEN_REPARSE_POINT on Windows; metadata checks are performed on the opened handle, avoiding a separate pre-open stat and its TOCTOU window. Reads are read-only and bounded to 4 KiB/64 PIDs, reject non-regular files, symlinks/reparse points, foreign-owned Linux files and hardlinks. Parsed values are used only to determine whether the corresponding pad should show its external-process LED; the file is not executed, modified, logged or returned.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- **Optional PID status path / baseline path-injection alerts:** the path is intentionally selectable by the user because scripts may write their PID file at a custom location. It is only opened read-only; parent traversal is rejected, the parent is canonicalized, and the final file is opened without following links/reparse points. Validation is based on metadata from the opened handle, so no separate pre-open stat is performed. The handle must refer to a regular, bounded, current-user file (including hardlink checks on Linux). Contents are parsed strictly as bounded numeric PIDs and affect only the pad's external-process LED. Confining it to a fixed application directory would break existing script workflows. The residual PR path-injection alert(s) on this flow should remain open for human review; do not dismiss them without recording this behavioral contract and confirming the latest analysis.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use; the static alert still needs fresh-scan review.

## Validation performed on this branch

Latest completed local Linux validation before this document was added:

- cargo fmt --all -- --check
- cargo test --workspace --locked
- cargo clippy --workspace --all-targets --locked -- -D warnings
- cargo build --workspace --release --locked

All passed on Nobara/Linux. The latest full run covered 129 tests: 19 app unit tests, 2 CLI integration tests, 28 core unit tests, 23 compatibility tests and 57 platform tests; formatting, strict Clippy and optimized release build also passed. Hosted CI on f6de3a6 passed Linux tests/Clippy/release build and GTK smoke. Windows tests exposed an intermittent PowerShell temp-script collision: all but the installer idempotency test passed. The collision-safe naming/ownership fix described above is being validated in the next hosted Windows run; do not call Windows acceptance complete until that run passes.

## Remaining acceptance

1. Push the reviewed changes on this existing PR branch and require the Linux and Windows Rust CI plus CodeQL to evaluate the exact proposed commit.
2. Review the new CodeQL alert set by source-to-sink trace. Do not bulk-dismiss test-path or intended-feature findings without confirming the full flow.
3. Confirm whether the bounded parser and Serde conversion clear the critical deserialization finding; if the static query still flags the path, inspect its complete flow before making any disposition. Review PATH-selected Crosshair findings against the trust model above.
4. Complete physical Launchpad MK1/MK2, KDE Wayland/PipeWire loopback, Windows WASAPI, UI/tray/input, side-by-side install/uninstall, config migration and rollback acceptance before production cutover.
5. Keep v2.4.0 as production until those gates pass.
