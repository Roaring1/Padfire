# Padfire Rust security remediation - 2026-11-23

## Status and boundaries

This is the integrated Rust security + Launchpad UI candidate branch based on main. It is not a release and it does not change the installed Padfire v2.4.0 GTK application, its config, audio routing, service state, or Launchpad integration. The reviewed UI commits formerly in draft PR #20 were cherry-picked onto PR #22; PR #20 is now closed as superseded. Keep v2.4.0 as the production fallback until the Rust candidate completes its separate security, hardware and install/uninstall acceptance.

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

After bounded parsing and schema normalization, the typed AppConfig conversion uses Serde's Deserialize implementation over the already-bounded Value tree. The untrusted byte stream is not passed to serde_json::from_slice/from_reader, and the code does not call the CodeQL-modeled serde_json::from_value sink. The previous combined-head PR CodeQL analysis on 137327d reported one high-severity path-injection result (#185) at runtime.rs:261; Python and Actions reported zero. The critical unsafe-deserialization finding was absent from that scan. On supply-chain follow-up head c1a52322431f26d803660fb9d2de29a83821258d, the explicit PR CodeQL analysis [45277537921](https://github.com/Roaring1/Padfire/actions/runs/45277537921) completed successfully for Rust, Python and Actions. However, the separate CodeQL policy check [138053461156](https://github.com/Roaring1/Padfire/runs/138053461156) failed with one new high-severity result at runtime.rs:261: alert #185. It remains open on the PR ref and was not dismissed. The repository Code Scanning API separately reports 104 open baseline alerts on refs/heads/main at commit 1cc3af4; these must not be confused with the PR-ref finding. No alert was dismissed.

### Dependency and license policy

- The locked dependency graph contained `url 2.5.8`, which cargo-deny reported against RUSTSEC-2026-0592 (stack-overflow advisory). The lockfile now pins the fixed `url 2.5.9` and its `form_urlencoded 1.2.3` dependency.
- Internal workspace path dependencies now include the exact `3.0.0-alpha.1` version constraint; cargo-deny no longer reports wildcard path dependencies.
- `rust/deny.toml` uses cargo-deny 0.22.0 and enforces RustSec/yanked advisories, an explicit SPDX license allowlist, wildcard bans, duplicate-version warnings and registry/git source provenance. The local full `cargo deny check` passes all four sections: advisories, licenses, bans and sources.
- The manifest inventory covered 457 locked packages, including 454 registry packages with checksums and parsed license metadata. Dual-license expressions are accepted where an explicitly allowed permissive option exists; GPL/LGPL-only expressions are not globally allowed.
- Duplicate transitive crate versions (including OS-specific Windows/macOS/Android dependencies) remain warnings for review, not suppressed or silently accepted as equivalent.
- The Rust GitHub Actions workflow now installs pinned cargo-deny 0.22.0 and runs `cargo deny check` on each Rust push/PR. The exact c1a5232 hosted dependency-policy jobs passed on both PR and push workflows.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
- The optional PID status-file path remains user-selectable because script authors may choose where their script writes the PID list. The resolver rejects parent traversal and canonicalizes the existing parent while preserving the final filename. It is opened directly with no-follow/nonblocking flags on Linux or OPEN_REPARSE_POINT on Windows; metadata checks are performed on the opened handle, avoiding a separate pre-open stat and its TOCTOU window. Reads are read-only and bounded to 4 KiB/64 PIDs, reject non-regular files, symlinks/reparse points, foreign-owned Linux files and hardlinks. Parsed values are used only to determine whether the corresponding pad should show its external-process LED; the file is not executed, modified, logged or returned.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- **Optional PID status path / PR alert #185:** the path remains user-selectable because script authors may choose where their script writes the PID list. It is opened read-only; parent traversal is rejected, the parent is canonicalized, and the final file is opened without following links/reparse points. Validation uses metadata from the opened handle, so no separate pre-open stat occurs. The handle must refer to a regular, bounded, current-user file (including hardlink checks on Linux). Contents are parsed as bounded numeric PIDs and affect only the pad's external-process LED. The current autoclick pad is configured to `/run/user/1000/autoclicker/autoclick.pid`, verified against the live KDE session's `XDG_RUNTIME_DIR` and `/home/roaring/autoclick.sh`; its previous `/tmp/autoclick.pid` value did not match the script. Confining this optional field to one fixed app directory would break other script workflows. Keep high-severity alert #185 open for human review; do not suppress it.
- **Per-user installer and desktop-integration roots:** `HOME`, `XDG_DATA_HOME`, `LOCALAPPDATA`, `APPDATA` and the optional `PADFIRE_START_MENU_DIR` value influence destinations inside the current user's profile. These are validated and canonicalized, with symlink/reparse-point ancestors rejected on Windows; the arbitrary `PADFIRE_INSTALL_ROOT` override is gone. They remain process-environment inputs, however, and their path flows should be reviewed individually. A same-user process that can already control the installer environment is within the current-user trust boundary, but do not mark a static path finding false-positive solely on that basis without checking the source-to-sink path.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use. The critical unsafe-deserialization finding is absent from the latest PR CodeQL scan.

## Validation performed on this branch

Latest local Nobara/Linux validation for the combined candidate head `c1a52322431f26d803660fb9d2de29a83821258d`:
- `cargo fmt --all -- --check`
- `cargo test --workspace --locked`: 140 passed, 0 failed (30 app, 2 CLI, 28 core, 23 compatibility, 57 platform)
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo build --workspace --release --locked`
- `cargo deny check` 0.22.0: advisories, licenses, bans and sources all passed
- `git diff --check`
- Legacy GTK/Python regression: 83 core tests and 7 dialog tests passed; Python bytecode, shell syntax, ShellCheck and desktop-entry validation passed.
- Isolated Xvfb render of the real Rust binary generated distinct Launchpad-grid and Settings screenshots; both local and hosted UI captures were visually inspected.

All hosted build/UI/dependency workflows for exact head `c1a5232` passed:
- Rust PR matrix: [45277552562](https://github.com/Roaring1/Padfire/actions/runs/45277552562); both OSs passed format, tests, strict Clippy and optimized release builds.
- Push Rust matrix: [45277537020](https://github.com/Roaring1/Padfire/actions/runs/45277537020); both OSs passed.
- cargo-deny dependency policy passed on both PR and push workflows.
- UI Linux render and screenshot artifact passed on both workflows.
- GTK legacy smoke: [45277552484](https://github.com/Roaring1/Padfire/actions/runs/45277552484), passed.
- Explicit PR CodeQL analysis: [45277537921](https://github.com/Roaring1/Padfire/actions/runs/45277537921), all three analyzers (Rust, Python, Actions) completed successfully.

However, the separate GitHub CodeQL policy check [138053461156](https://github.com/Roaring1/Padfire/runs/138053461156) **failed** because the PR adds one high-severity path-injection alert (#185) at `rust/apps/padfire/src/runtime.rs:261`. The alert is on PR head `c1a5232`, and it has not been dismissed or fixed. The explicit analysis job passing means analysis completed; it does not override the failed alert policy check.

The broader main-branch Code Scanning baseline remains 104 open alerts (100 high path-injection, 3 critical command-line-injection and 1 critical unsafe-deserialization). No alert has been bulk-dismissed. The critical deserialization and arbitrary installer-root override findings were absent from the previous PR analysis.

## Nobara side-by-side installation

The latest Rust candidate is installed on Nobara at `~/.local/opt/padfire-rust/padfire-rust`, version `3.0.0-alpha.1`. Its SHA-256 is `02f6c890ed582bd22c94336d3564413e6287ff2a856348bc95d25172ae44be98`, matching the local release artifact with `url 2.5.9`. The `Padfire Rust` desktop entry and SVG icon were regenerated and validated. Previous candidate binaries and UI files are backed up under `~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/` and `.../2026-11-23-pre-url-2.5.9/`, both with verified manifests.

Config namespaces:
- Rust runtime: `~/.config/padfire-rust/padfire.json` (v5, 8 pages, 21 assignments; current SHA-256 `5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60`).
- Source/default candidate: `~/.config/padfire/padfire.json` (current SHA-256 `a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0`).
- Both candidate configs had the stale clicker PID path corrected from `/tmp/autoclick.pid` to `/run/user/1000/autoclicker/autoclick.pid` after verifying the live KDE session's `XDG_RUNTIME_DIR` and `/home/roaring/autoclick.sh`. Pre-change copies are backed up under `~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/` with a verified manifest. The autoclick script was not launched.
- Legacy v2.4.0 config: `~/.config/roaring/padfire.json` (unchanged SHA-256 `50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5`).

The Rust candidate is not running in the live desktop. No physical MIDI/audio state was changed; the legacy v2.4.0 launcher, config, audio routing and desktop entry remain unchanged.

## Remaining acceptance

1. Keep PR #22 open for human review of the one current high-severity PID-file path finding (#185 at runtime.rs:261); the separate PR CodeQL policy check remains red until that finding has a reviewed disposition.
2. Continue individual source-to-sink triage of the separate 104-alert main baseline. Do not bulk-dismiss findings; update issue #21 only with evidence-backed dispositions.
3. Continue reviewing user-authored shell scripts and PATH-selected Crosshair execution against their documented trust boundaries.
4. Review the cargo-deny duplicate-version warnings and add signing/release provenance checks; dependency/license/advisory policy is now configured and passing.
5. Complete physical Launchpad MK1/MK2, KDE Wayland/PipeWire loopback, Windows WASAPI, UI/tray/input, side-by-side install/uninstall, config migration and rollback acceptance before production cutover.
6. Keep v2.4.0 as production until all gates in CUTOVER.md pass.
