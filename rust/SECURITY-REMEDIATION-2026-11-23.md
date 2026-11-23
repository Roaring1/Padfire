# Padfire Rust security remediation - 2026-11-23

## Status and boundaries

This is an isolated security-remediation branch based on main. It is not a release and it does not change the installed Padfire v2.4.0 GTK application, its config, audio routing, service state, or Launchpad integration. Keep v2.4.0 as the production fallback until the Rust candidate completes its separate hardware and install/uninstall acceptance.

The repository's CodeQL dashboard was inspected before these changes. It showed 104 open Rust alerts:

- 100 rust/path-injection alerts (high).
- 3 rust/command-line-injection alerts (critical).
- 1 rust/unsafe-deserialization alert (critical).

Those are baseline counts on main, not the result for the current security PR. No alerts have been dismissed as part of this work. A fresh PR CodeQL result is recorded below: one high-severity PID status-file path finding remains open; the critical deserialization finding is absent from the PR analysis.

## Changes made

### Filesystem paths and uninstall safety

- Uninstall no longer recursively deletes the configured install root. It removes only Padfire's known executable, desktop entry/shortcut and icon, then removes the install directory only if it is empty.
- The Windows delayed-uninstall script now deletes only padfire-rust.exe and attempts a non-recursive empty-directory removal. Other contents are retained.
- A directory or symlink where a Padfire file is expected is not recursively removed.
- The install root uses the conventional per-user profile locations (`$HOME/.local/opt/padfire-rust` on Linux and `%LOCALAPPDATA%/Programs/Padfire Rust` on Windows). The arbitrary `PADFIRE_INSTALL_ROOT` override was removed. These roots are still derived from the running process environment; this change does not replace `HOME` or `LOCALAPPDATA` with account-database/Known Folder lookups.
- Desktop integration paths remain environment-selected: Linux uses `XDG_DATA_HOME` or `HOME`, and Windows uses `APPDATA` with the optional `PADFIRE_START_MENU_DIR` override. `HOME`, `XDG_DATA_HOME`, `APPDATA`, `LOCALAPPDATA` and `PADFIRE_START_MENU_DIR` are validated as absolute paths without parent traversal. The nearest existing ancestor is canonicalized before descendants are formed. Windows rejects symlink/reparse-point ancestors, then removes only the extended-length prefix from the physical path before passing it to PowerShell/COM. Treat these environment values as a documented current-user trust boundary, not as OS-derived identities.
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

After bounded parsing and schema normalization, the typed AppConfig conversion uses Serde's Deserialize implementation over the already-bounded Value tree. The untrusted byte stream is not passed to serde_json::from_slice/from_reader, and the code does not call the CodeQL-modeled serde_json::from_value sink. The PR CodeQL workflow on cb0a168 completed successfully for Rust, Python and Actions. Rust reported one high-severity path-injection result (#185) at runtime.rs:261; Python and Actions reported zero. The separate PR CodeQL policy check is red because that finding remains open. The critical unsafe-deserialization finding is absent from the current PR analysis. The repository Code Scanning API separately reports 104 open baseline alerts on refs/heads/main at commit 1cc3af4; these must not be confused with the single current PR-ref finding. No alert was dismissed.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
- The optional PID status-file path remains user-selectable because script authors may choose where their script writes the PID list. The resolver rejects parent traversal and canonicalizes the existing parent while preserving the final filename. It is opened directly with no-follow/nonblocking flags on Linux or OPEN_REPARSE_POINT on Windows; metadata checks are performed on the opened handle, avoiding a separate pre-open stat and its TOCTOU window. Reads are read-only and bounded to 4 KiB/64 PIDs, reject non-regular files, symlinks/reparse points, foreign-owned Linux files and hardlinks. Parsed values are used only to determine whether the corresponding pad should show its external-process LED; the file is not executed, modified, logged or returned.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- **Optional PID status path / PR alert #185:** the path is intentionally selectable by the user because scripts may write their PID file at a custom location. It is opened read-only; parent traversal is rejected, the parent is canonicalized, and the final file is opened without following links/reparse points. Validation uses metadata from the opened handle, so no separate pre-open stat occurs. The handle must refer to a regular, bounded, current-user file (including hardlink checks on Linux). Contents are parsed as bounded numeric PIDs and affect only the pad's external-process LED. Confining it to a fixed application directory would break existing script workflows. Keep the one high-severity PR alert open for human review; do not suppress it.
- **Per-user installer and desktop-integration roots:** `HOME`, `XDG_DATA_HOME`, `LOCALAPPDATA`, `APPDATA` and the optional `PADFIRE_START_MENU_DIR` value influence destinations inside the current user's profile. These are validated and canonicalized, with symlink/reparse-point ancestors rejected on Windows; the arbitrary `PADFIRE_INSTALL_ROOT` override is gone. They remain process-environment inputs, however, and their path flows should be reviewed individually. A same-user process that can already control the installer environment is within the current-user trust boundary, but do not mark a static path finding false-positive solely on that basis without checking the source-to-sink path.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use. The critical unsafe-deserialization finding is absent from the latest PR CodeQL scan.

## Validation performed on this branch

Latest local Nobara/Linux validation on cb0a168:
- cargo fmt --all -- --check
- cargo test --workspace --locked: 129 passed, 0 failed (19 app, 2 CLI, 28 core, 23 compatibility, 57 platform)
- cargo clippy --workspace --all-targets --locked -- -D warnings
- cargo build --workspace --release --locked

Hosted CI for cb0a168 passed:
- Ubuntu: 129 tests (19 app, 2 CLI, 28 core, 23 compatibility, 57 platform), strict Clippy and optimized release build.
- Windows: 133 tests (19 app, 2 CLI, 28 core, 23 compatibility, 61 platform), strict Clippy and optimized release build. This includes PowerShell temp-file collision ownership and direct -File execution regressions.
- GTK smoke: run 45170215775 passed.
- Rust/Actions/Python CodeQL: run 45170207634 completed with result counts 1/0/0. The Rust result is the one high-severity PID-file path finding (#185, runtime.rs:261); the separate PR CodeQL policy check is red because that finding remains open.

The broader main-branch Code Scanning baseline remains 104 open alerts (100 high path-injection, 3 critical command-line-injection and 1 critical unsafe-deserialization). No alert has been bulk-dismissed. The critical deserialization and arbitrary installer-root findings are absent from the current PR scan.

## Remaining acceptance

1. Keep PR #22 open for human review of the one current high-severity PID-file path finding (#185 at runtime.rs:261); the separate PR CodeQL policy check remains red until that finding has a reviewed disposition.
2. Continue individual source-to-sink triage of the separate 104-alert main baseline. Do not bulk-dismiss findings; update issue #21 only with evidence-backed dispositions.
3. Continue reviewing user-authored shell scripts and PATH-selected Crosshair execution against their documented trust boundaries.
4. Add dependency/license policy and provenance checks; they are not currently configured and remain production release gates.
5. Complete physical Launchpad MK1/MK2, KDE Wayland/PipeWire loopback, Windows WASAPI, UI/tray/input, side-by-side install/uninstall, config migration and rollback acceptance before production cutover.
6. Keep v2.4.0 as production until all gates in CUTOVER.md pass.
