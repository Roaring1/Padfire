# Padfire Rust security remediation - 2026-11-23

## Status and boundaries

This is the integrated Rust security + Launchpad UI candidate branch based on main. It is not a release and it does not change the installed Padfire v2.4.0 GTK application, its config, audio routing, service state, or Launchpad integration. The reviewed UI commits formerly in draft PR #20 were cherry-picked onto PR #22; PR #20 is now closed as superseded. Keep v2.4.0 as the production fallback until the Rust candidate completes its separate security, hardware and install/uninstall acceptance.

The repository's CodeQL dashboard was inspected before these changes. It showed 104 open Rust alerts:

- 100 rust/path-injection alerts (high).
- 3 rust/command-line-injection alerts (critical).
- 1 rust/unsafe-deserialization alert (critical).

Those are baseline counts on main, not the result for the current security PR. No alerts have been dismissed as part of this work. The final integrated 49311c3 PR CodeQL result is green: explicit Rust, Python and Actions SARIF each report zero findings, and the changed-file policy says “No new alerts in code changed by this pull request.” The PR ref has zero open alert instances. Earlier #185 and installer path findings are historical data on older PR heads and were not dismissed. The separate 104-alert main baseline remains open pending merge and a fresh main scan.

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

After bounded parsing and schema normalization, typed AppConfig conversion uses Serde's Deserialize implementation over the already-bounded Value tree; untrusted bytes are not sent to serde_json::from_slice/from_reader or the modeled serde_json::from_value sink. Historical CodeQL on 137327d and c1a5232 reported a PID-path finding (#185), followed by installer path findings on 8c625a3/77b40fe. Those code paths were remediated: Unix PID access is descriptor-relative, executable copying uses the opened process-image handle, and path-opening test adapters were removed. The final integrated head 49311c3 has zero Rust/Python/Actions findings and zero open PR-ref alerts. The separate main branch still contains 104 pre-merge baseline alerts; no alert was dismissed or bulk-cleared.

### Dependency and license policy

- The locked dependency graph contained `url 2.5.8`, which cargo-deny reported against RUSTSEC-2026-0592 (stack-overflow advisory). The lockfile now pins the fixed `url 2.5.9` and its `form_urlencoded 1.2.3` dependency.
- Internal workspace path dependencies now include the exact `3.0.0-alpha.1` version constraint; cargo-deny no longer reports wildcard path dependencies.
- `rust/deny.toml` uses cargo-deny 0.22.0 and enforces RustSec/yanked advisories, an explicit SPDX license allowlist, wildcard bans, duplicate-version warnings and registry/git source provenance. The local full `cargo deny check` passes all four sections: advisories, licenses, bans and sources.
- The manifest inventory covered 457 locked packages, including 454 registry packages with checksums and parsed license metadata. Dual-license expressions are accepted where an explicitly allowed permissive option exists; GPL/LGPL-only expressions are not globally allowed.
- Duplicate transitive crate versions (including OS-specific Windows/macOS/Android dependencies) remain warnings for review, not suppressed or silently accepted as equivalent.
The Rust GitHub Actions workflow installs pinned cargo-deny 0.22.0 and runs the full advisory/license/ban/source policy on each Rust push/PR. The current 49311c3 push and pull-request dependency-policy jobs both pass, including pinned cargo-about 0.9.3 and the offline byte-for-byte third-party notice regeneration check.

### Third-party notice packaging

A 465 KB HTML license report is generated at `rust/THIRD_PARTY_LICENSES.html` using cargo-about 0.9.3 and the exact Cargo.lock. It includes 82 full license-text sections and 311 crate attribution links for 296 distinct crate/version labels across the Linux x86-64 and Windows x86-64 release targets, including runtime/build/dev/transitive dependencies. `rust/about.toml` defines the accepted SPDX licenses to stay aligned with `rust/deny.toml`; `rust/about.hbs` supplies the readable HTML format. `--fail` ensures unrecognized license expressions stop generation.

The installer embeds this report and treats it as a managed install asset. Linux installs to `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html`; Windows installs beside `padfire-rust.exe`. Existing report bytes participate in the same backup/restore transaction as desktop integration and the executable. Uninstall removes only the app-owned notice, and deferred Windows uninstall retries removal after the running executable unlocks. Installer tests verify repeat-install, byte-exact payload, rollback restoration, ordinary uninstall, and deferred Windows cleanup. The dependency-policy CI job fetches all locked target crates, installs pinned cargo-about 0.9.3, regenerates the notice offline with `--fail`, then byte-compares the result against the checked-in HTML.

The production executable-copy path avoids reopening the running executable by a caller-provided pathname. Linux uses fixed /proc/self/exe, verifies its opened handle metadata as a regular file, and copies through a cloned handle. The 77b40fe scan reduced the earlier findings to one test-only path adapter; 1100c95 removed both path-opening install/copy test adapters entirely. Installer tests use the already-open process-image handle. The final integrated head 49311c3 passes the full 144-test workspace suite, strict Clippy, release build, cargo-deny and deterministic offline notice generation/byte comparison. Windows cloned file handles share their cursor, so each copy now seeks to byte zero before any attempt; a regression exercises failed publication followed by retry with the same handle.

The notice-bearing installer is now installed on Nobara at ~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html. Its hash matches the generated report byte-for-byte. Hosted dependency-policy jobs pass the cargo-about regeneration gate on the exact 49311c3 source head.

### Process and IPC boundaries

- Crosshair discovery now canonicalizes PATH and user-local candidates and returns only an absolute regular executable (and, on Unix, one with executable permission).
The optional PID status-file path remains user-selectable because script authors may choose where their script writes the PID list. Parent traversal is rejected and the existing parent is canonicalized. On Unix, the current implementation walks each parent component by directory descriptor using openat with O_DIRECTORY and O_NOFOLLOW, then opens the final filename relative to the verified parent descriptor with O_NOFOLLOW and O_NONBLOCK. This prevents parent/final symlink traversal and removes the prior absolute-path open sink. Windows retains FILE_FLAG_OPEN_REPARSE_POINT and opened-handle metadata. Reads are read-only and bounded to 4 KiB/64 PIDs, reject non-regular files, symlinks/reparse points, foreign-owned Linux files and hardlinks. The current autoclick path is /run/user/1000/autoclicker/autoclick.pid, verified against /home/roaring/autoclick.sh. A regression rejects symlinked parent components while valid canonicalized custom paths remain supported.
- Unix IPC socket paths must be absolute, cannot contain parent-directory traversal, and reject symlink/non-directory parent components. Both server bind and client connect use the canonical parent path. The socket itself remains owner-only.
- Loudness analysis resolves an existing local regular file to an absolute path and restricts FFmpeg input protocols to file, preventing network protocol input through that analysis path.

## Findings that require an explicit trust decision

- **Optional PID status path / historical alert #185:** the path remains user-selectable for script authors. The current Unix implementation uses a root-anchored descriptor-relative openat walk with O_NOFOLLOW on every directory component and the final filename, followed by opened-handle regular-file/owner/hardlink/size checks; Windows retains reparse-point handle checks. The current autoclick path is /run/user/1000/autoclicker/autoclick.pid. Explicit Rust CodeQL on 49311c3 reports zero findings, the changed-file policy is green, and the current PR ref has zero open alerts. Historical #185 was not dismissed; it is no longer introduced by the PR diff. Do not suppress future regressions in this flow.
- **Per-user installer and desktop-integration roots:** `HOME`, `XDG_DATA_HOME`, `LOCALAPPDATA`, `APPDATA` and the optional `PADFIRE_START_MENU_DIR` value influence destinations inside the current user's profile. These are validated and canonicalized, with symlink/reparse-point ancestors rejected on Windows; the arbitrary `PADFIRE_INSTALL_ROOT` override is gone. They remain process-environment inputs, however, and their path flows should be reviewed individually. A same-user process that can already control the installer environment is within the current-user trust boundary, but do not mark a static path finding false-positive solely on that basis without checking the source-to-sink path.
- Custom script actions: Padfire deliberately supports user-authored shell scripts. On Unix they are run by /bin/sh -c; on Windows they are passed to cmd.exe /C. This feature is intentionally powerful and is not sandboxed. The config store is user-owned/private, but same-user local processes remain within the trust boundary. A future sandbox/confirmation policy would be a product design change and must not silently remove existing script functionality.
- Deserialization: retain extension fields by design, so deny_unknown_fields is not compatible with the v5 forward-compatibility contract. The byte, depth, node, page, pad and field bounds mitigate unbounded resource use. The critical unsafe-deserialization finding is absent from the latest PR CodeQL scan.

## Final CodeQL result on 49311c3

Explicit Rust SARIF analysis 2235814466, Python 2235797264 and Actions 2235796428 all report 0 findings on exact head `49311c3a80c71484fd86afd3edb9ee667cea8826`. GitHub changed-file policy check 138196612506 is successful: “No new alerts in code changed by this pull request.” The current PR #22 ref contains 0 open Code Scanning alert instances. The main baseline still has 104 open findings (100 high path injection, 3 critical command-line injection and 1 critical unsafe deserialization) on pre-merge main; issue #21 remains open pending merge and a fresh main scan.

## Validation performed on this branch

Latest local Nobara/Linux validation for 49311c3a80c71484fd86afd3edb9ee667cea8826:
- cargo fmt --all -- --check: pass
- cargo test --workspace --locked: 144 passed, 0 failed (31 app, 2 CLI, 28 core, 23 compatibility, 60 platform)
- cargo clippy --workspace --all-targets --locked -- -D warnings: pass
- cargo build --workspace --release --locked: pass
- cargo deny check 0.22.0: advisories, licenses, bans and sources all passed
- git diff --check: pass
- Focused PID-path tests: 9 passed, including symlinked-parent rejection
- Legacy GTK/Python regression: 83 core tests and 7 dialog tests passed; Python compile, shell syntax, ShellCheck and desktop-entry validation passed
- Isolated Xvfb main-grid and Settings screenshots from the initial c53 review were inspected; final 49311c3 hosted screenshots (540x586) were downloaded and visually reviewed under /home/roaring/Workspace/PadfireReview-2026-11-23/hosted-49311c3/.
- Disposable installer lifecycle: first install, repeat install, --status, uninstall, and byte-identical preservation of an existing v5 config all passed.

Hosted validation for exact 49311c3 head:
- Push Rust workflow [45320135973](https://github.com/Roaring1/Padfire/actions/runs/45320135973): Ubuntu and Windows format/test/strict Clippy/release, hosted UI render, dependency policy and notice regeneration passed.
- Pull-request Rust workflow [45320142859](https://github.com/Roaring1/Padfire/actions/runs/45320142859): Ubuntu/Windows matrix, UI, cargo-deny and notice report checks passed.
- PR CodeQL workflow [45320137717](https://github.com/Roaring1/Padfire/actions/runs/45320137717): explicit Rust/Python/Actions analyses passed; SARIF result counts were 0/0/0.
- GitHub CodeQL changed-file policy [138196612506](https://github.com/Roaring1/Padfire/runs/138196612506): success, ‘No new alerts in code changed by this pull request.’ Current PR-ref inventory: 0 open alert instances.
- Separate main Code Scanning baseline remains 104 open alerts (100 high path injection, 3 critical command-line injection, 1 critical unsafe deserialization), still tracked in issue #21. No baseline alerts were bulk-dismissed.

The previous high-severity PID-path findings are no longer introduced by the current changed-file diff. Their historical instances remain on earlier PR heads and were not dismissed. The broader main Code Scanning baseline remains 104 open alerts (100 high path injection, 3 critical command-line injection, 1 critical unsafe deserialization), tracked separately in issue #21.

## Nobara side-by-side installation

The current candidate is installed at `~/.local/opt/padfire-rust/padfire-rust`, version `Padfire 3.0.0-alpha.1`. Binary SHA-256 `e3543668e54ea335cedfd8d43afc84b594d71ed1f319ad3afb5b3aa155bdc6aa` exactly matches the local 49311c3 release artifact. Third-party notice at `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html` has SHA-256 `34cc307a6726f1d6bc43d7de6aab3ec2eb9db7463fe8b21f0cbf52b61542d06b`, matching the checked-in HTML byte-for-byte. Padfire Rust desktop entry and icon validate; KDE cache was refreshed. The warning about an unrelated pre-existing malformed `packettracer.desktop` persists; the Padfire entry itself is valid.

A final private `gtk-launch` smoke started the installed desktop entry under D-Bus/Xvfb with hardware disabled, returned expected status, then quit and removed the IPC socket. A private runtime smoke loaded a copy of the actual v5 Rust config and confirmed page 0, monitor enabled and sink `pulseaudio:vm_music`; reload/show/quit passed and the source config stayed unchanged. The real candidate is stopped (`running:false`) with no process or stale socket. `file`, `ldd` and `ldd -r` confirm native x86-64 ELF and resolved runtime libraries/symbols.

Rollback sets with verified SHA-256 manifests:
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-url-2.5.9/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-c53-openat/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-49311c3/` (prior c53 binary/desktop/icon)
- `~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/` (pre-change candidate config)

Rust runtime config remains v5 at `~/.config/padfire-rust/padfire.json` (SHA-256 `5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60`). Source/default config remains v5 at `~/.config/padfire/padfire.json` (SHA-256 `a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0`). Both use `/run/user/1000/autoclicker/autoclick.pid`. Legacy v2.4.0 config remains unchanged (SHA-256 `50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5`), as do its launcher and desktop entry.

The old Oct 31 legacy autostart desktop and systemd user unit remain untouched; both target the v2.4.0 launcher. No Rust autostart entry or service was created. Physical Launchpad/LPD8, visible KDE/Windows tray, real PipeWire/WASAPI playback/monitor/VU, dual-machine acceptance and release signing/provenance remain manual production gates. No live MIDI port, Carla session or PipeWire route was opened/changed during this update.

## Post-merge CodeQL reconciliation - 2026-11-24

PR #22 was merged into `main` as `986cc14946392150578fae97a9cd865864eaea39`. The first fresh main-branch CodeQL analysis completed successfully and reduced the old 104-alert inventory to four current findings on that merge commit:

- Alert #16, critical `rust/command-line-injection`, at `process_manager.rs:475`. The reported source was the explicit Unix `HOME/.local/bin/crosshair` fallback at line 776. The sink is `Command::new(&executable)`; the selected path is canonicalized, and the only argument is the fixed literal `--toggle`. It does not pass through a shell.
- Alerts #115 and #57, high `rust/path-injection`, at `process_manager.rs:812-813`. Both came from that same HOME-derived candidate and were limited to regular-file and executable-permission metadata checks before process launch.
- Alert #75, high `rust/path-injection`, at `apps/padfire/tests/cli.rs:20`. The test helper constructed a directory from `std::env::temp_dir()` and passed it to `create_dir_all`.

These findings were reviewed individually rather than bulk-dismissed. The source changes remove the redundant Unix HOME fallback (the supervised user-session PATH is verified to include `~/.local/bin`) and use the existing canonicalized PATH lookup, retaining Windows' LOCALAPPDATA fallback. The CLI tests now use `tempfile::TempDir` for exclusive temporary-directory creation and automatic cleanup. No alert was dismissed to obtain the reduced count.

Local verification of this follow-up: `cargo fmt --all -- --check`, the full 144-test workspace suite, strict Clippy with `-D warnings`, optimized release build, and `git diff --check` all pass. The follow-up must be pushed and receive a fresh main CodeQL scan before issue #21 can be closed; do not treat the merge-commit scan as confirmation of the subsequent code change.

## Remaining acceptance

1. Verify the fresh main CodeQL scan after the 2026-11-24 follow-up and confirm each affected alert is fixed; keep issue #21 open until then.
2. Continue reviewing user-authored shell-script execution against the documented same-user trust boundary.
3. Review cargo-deny duplicate-version warnings and add signing/release provenance checks; dependency/license/advisory policy is configured and passing.
4. Complete physical Launchpad MK1/MK2 input/LED/reconnect, KDE Wayland/PipeWire playback and loopback, Windows WASAPI, visible UI/tray, install/uninstall, config migration and rollback acceptance before production release.
5. Keep v2.4.0 as the rollback until all gates in CUTOVER.md pass.
