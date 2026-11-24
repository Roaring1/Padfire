# Padfire Rust security remediation - 2026-11-23

## Status and boundaries

This is the integrated Rust security + Launchpad UI candidate branch based on main. It is not a release and it does not change the installed Padfire v2.4.0 GTK application, its config, audio routing, service state, or Launchpad integration. The reviewed UI commits formerly in draft PR #20 were cherry-picked onto PR #22; PR #20 is now closed as superseded. Keep v2.4.0 as the production fallback until the Rust candidate completes its separate security, hardware and install/uninstall acceptance.

The repository's CodeQL dashboard was inspected before these changes. It showed 104 open Rust alerts:

- 100 rust/path-injection alerts (high).
- 3 rust/command-line-injection alerts (critical).
- 1 rust/unsafe-deserialization alert (critical).

Those are baseline counts on main, not the result for the current security PR. No alerts have been dismissed as part of this work. The latest c53 PR CodeQL policy result is green with no new alerts in changed files. The earlier #185 PID status-file instance remains as historical Code Scanning data on an older PR head and was not dismissed. The critical unsafe-deserialization finding is absent from the current PR analysis. The separate 104-alert main baseline is still open.

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

### Third-party notice packaging

A 465 KB HTML license report is generated at `rust/THIRD_PARTY_LICENSES.html` using cargo-about 0.9.3 and the exact Cargo.lock. It includes 82 full license-text sections and 311 crate attribution links for 296 distinct crate/version labels across the Linux x86-64 and Windows x86-64 release targets, including runtime/build/dev/transitive dependencies. `rust/about.toml` defines the accepted SPDX licenses to stay aligned with `rust/deny.toml`; `rust/about.hbs` supplies the readable HTML format. `--fail` ensures unrecognized license expressions stop generation.

The installer embeds this report and treats it as a managed install asset. Linux installs to `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html`; Windows installs beside `padfire-rust.exe`. Existing report bytes participate in the same backup/restore transaction as desktop integration and the executable. Uninstall removes only the app-owned notice, and deferred Windows uninstall retries removal after the running executable unlocks. Installer tests verify repeat-install, byte-exact payload, rollback restoration, ordinary uninstall, and deferred Windows cleanup. The dependency-policy CI job fetches all locked target crates, installs pinned cargo-about 0.9.3, regenerates the notice offline with `--fail`, then byte-compares the result against the checked-in HTML.

The production executable-copy path was subsequently changed to avoid reopening the running executable by a caller-provided pathname. Linux uses the fixed `/proc/self/exe` process-image handle, verifies its metadata as a regular file, and copies via a cloned handle. The `77b40fe` scan reduced the two initial path findings to one at a test-only adapter; the path-opening install/copy test adapters have since been removed, and tests use the opened process-image helper. The latest local 143-test workspace suite, strict Clippy, release build, cargo-deny and offline notice regeneration/byte comparison all pass. A fresh CodeQL result for the adapter-free tree is pending; keep the path findings unresolved until the current changed-file policy passes.

The generated notice and installer integration are currently under fresh local validation; the c53 binary already installed on Nobara does not yet contain this new asset. Update the side-by-side installation only after exact-head Ubuntu/Windows CI passes.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
- The optional PID status-file path remains user-selectable because script authors may choose where their script writes the PID list. Parent traversal is rejected and the existing parent is canonicalized. On Unix, the c53 implementation starts from the fixed filesystem root, walks each parent component by directory descriptor using openat with O_DIRECTORY and O_NOFOLLOW, then opens the final filename relative to the validated parent descriptor with O_NOFOLLOW and O_NONBLOCK. This avoids passing the user-selected absolute path directly to the file-open sink and prevents parent/final symlink traversal during the descriptor walk. Windows continues to use FILE_FLAG_OPEN_REPARSE_POINT and opened-handle metadata. Reads are read-only and bounded to 4 KiB/64 PIDs, reject non-regular files, symlinks/reparse points, foreign-owned Linux files and hardlinks. Parsed values are used only to determine whether the corresponding pad should show its external-process LED; the file is not executed, modified, logged or returned. A focused regression rejects symlinked parent components while preserving normal canonicalized custom paths.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- **Optional PID status path / historical PR alert #185:** the path remains user-selectable because script authors may choose where their script writes the PID list. The c53 Unix implementation uses a root-anchored descriptor-relative openat walk with O_NOFOLLOW on every directory component and the final filename, followed by opened-handle regular-file/owner/hardlink/size checks. Windows retains its reparse-point handle checks. The current autoclick pad is configured to /run/user/1000/autoclicker/autoclick.pid, verified against the live KDE session and /home/roaring/autoclick.sh. The explicit CodeQL analyses for c53 passed and CodeQL policy check 138094831040 reports “No new alerts in code changed by this pull request.” The older #185 instance is still historical Code Scanning data on an earlier PR head; it was not dismissed. Do not suppress future regressions in this flow.
- **Per-user installer and desktop-integration roots:** `HOME`, `XDG_DATA_HOME`, `LOCALAPPDATA`, `APPDATA` and the optional `PADFIRE_START_MENU_DIR` value influence destinations inside the current user's profile. These are validated and canonicalized, with symlink/reparse-point ancestors rejected on Windows; the arbitrary `PADFIRE_INSTALL_ROOT` override is gone. They remain process-environment inputs, however, and their path flows should be reviewed individually. A same-user process that can already control the installer environment is within the current-user trust boundary, but do not mark a static path finding false-positive solely on that basis without checking the source-to-sink path.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use. The critical unsafe-deserialization finding is absent from the latest PR CodeQL scan.

## Validation performed on this branch

Latest local Nobara/Linux validation for c53c3ea757f01456e80ca0a13220929e3bebe4f4:
- cargo fmt --all -- --check: pass
- cargo test --workspace --locked: 143 passed, 0 failed (31 app, 2 CLI, 28 core, 23 compatibility, 59 platform)
- cargo clippy --workspace --all-targets --locked -- -D warnings: pass
- cargo build --workspace --release --locked: pass
- cargo deny check 0.22.0: advisories, licenses, bans and sources all passed
- git diff --check: pass
- Focused PID-path tests: 9 passed, including symlinked-parent rejection
- Legacy GTK/Python regression: 83 core tests and 7 dialog tests passed; Python compile, shell syntax, ShellCheck and desktop-entry validation passed
- Isolated Xvfb main-grid and Settings renders were visually inspected (540x480); screenshots saved to /home/roaring/Workspace/PadfireReview-2026-11-23/c53c3ea/.
- Disposable installer lifecycle: first install, repeat install, --status, uninstall, and byte-identical preservation of an existing v5 config all passed.

Hosted validation for exact c53 head:
- PR run [45289561400](https://github.com/Roaring1/Padfire/actions/runs/45289561400): completed successfully; explicit Rust/Python/Actions CodeQL analyses passed.
- GitHub CodeQL policy check [138094831040](https://github.com/Roaring1/Padfire/runs/138094831040): success, “No new alerts in code changed by this pull request.”
- Push Rust run [45289559703](https://github.com/Roaring1/Padfire/actions/runs/45289559703): Ubuntu and Windows build/test/Clippy/release matrix passed.
- PR Rust run [45289566438](https://github.com/Roaring1/Padfire/actions/runs/45289566438): Ubuntu/Windows matrix passed.
- cargo-deny, hosted UI screenshot jobs and GTK legacy smoke all passed on c53.

The previous high-severity PID-path finding #185 is no longer introduced by the current changed-file diff. Its historical instance remains in Code Scanning on an earlier PR head and was not dismissed. The broader main Code Scanning baseline remains 104 open alerts (100 high path injection, 3 critical command-line injection, 1 critical unsafe deserialization), tracked separately in issue #21.

## Nobara side-by-side installation

The latest Rust candidate is installed at ~/.local/opt/padfire-rust/padfire-rust, version 3.0.0-alpha.1. SHA-256 aef9169b9ff23e96091c14f7d36930a03b53329afcbd984675e0764e9343ba5b exactly matches the local c53 release artifact. Padfire Rust desktop entry and SVG icon validate; KDE database/service cache was refreshed. The refresh reports an unrelated pre-existing malformed packettracer.desktop file; the Padfire entry itself validates.

Rollback candidate binary/desktop/icon sets, all with verified SHA-256 manifests:
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-url-2.5.9/
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-c53-openat/

Config namespaces:
- Rust runtime: ~/.config/padfire-rust/padfire.json (v5, 8 pages, 21 assignments; SHA-256 5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60).
- Source/default candidate: ~/.config/padfire/padfire.json (v5; SHA-256 a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0).
- Both candidate configs use /run/user/1000/autoclicker/autoclick.pid, matching the live KDE runtime and /home/roaring/autoclick.sh. Their pre-change copies are backed up under ~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/ with a verified manifest. The script was not launched.
- Legacy v2.4.0 config: ~/.config/roaring/padfire.json (unchanged SHA-256 50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5).

A disposable HOME/XDG installer test confirmed repeat-install and uninstall preserve an existing v5 config byte-for-byte. The candidate is stopped, with no Padfire Rust process or stale socket. Legacy v2.4.0 launcher/config/desktop entry remain unchanged.

## Remaining acceptance

1. Keep PR #22 open for final review; c53 explicit CodeQL analysis and the changed-file CodeQL policy check pass with no new alerts. The older #185 instance remains historical and was not dismissed.
2. Continue individual source-to-sink triage of the separate 104-alert main baseline. Do not bulk-dismiss findings; issue #21 remains open.
3. Continue reviewing user-authored shell scripts and PATH-selected Crosshair execution against their documented trust boundaries.
4. Review cargo-deny duplicate-version warnings and add signing/release provenance checks; dependency/license/advisory policy is configured and passing.
5. Complete physical Launchpad MK1/MK2, KDE Wayland/PipeWire loopback, Windows WASAPI, UI/tray/input, side-by-side install/uninstall, config migration and rollback acceptance before production cutover.
6. Keep v2.4.0 as production until all gates in CUTOVER.md pass.
