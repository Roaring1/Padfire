# Padfire Rust rewrite handoff

Updated: 2026-11-23 (America/Los_Angeles)

## Checkpoint

- Repository: `Roaring1/Padfire`
- Production reference: released Padfire v2.4.0 on `main`; current security branch is based on `1cc3af4ebc7483c29c23e1bd7f41932948a99510`.
- Active security branch: `security/triage-codeql-paths-2026-11-23`
- Latest combined security + UI + supply-chain commit: `c53c3ea757f01456e80ca0a13220929e3bebe4f4` (descriptor-relative Unix PID status-file open).
- Active integrated candidate PR #22: https://github.com/Roaring1/Padfire/pull/22 (open, non-draft, unmerged).
- The former UI-only draft PR #20 was closed as superseded after its four reviewed commits were cherry-picked onto PR #22.
- Production v2.4.0 remains the reference; the Rust alpha is installed side-by-side on Nobara only, with no production cutover.
- Nobara checkout: `/home/roaring/Documents/GitHub/Padfire`
- Windows checkout: `%USERPROFILE%\src\Padfire` on SSH alias `laptop`

Do not modify the production launcher, replace v2.4.0, change live audio routing/configuration, or claim physical MIDI hardware during unattended development.

## Current implementation

The Rust workspace contains `padfire-core`, `padfire-platform`, and a native eframe/egui desktop app. It includes v1-v5 config compatibility, generation-safe pad actions, MIDI protocol/reconnect, native audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, tray controls, cancellable FFmpeg LUFS analysis, playback RMS metering, and per-user side-by-side install/uninstall.

- Tray menu: Show, Stop Audio, Quit; code-generated icon; initialized on the UI thread.
- Window lifecycle: titlebar Close hides via `Visible(false)` with `CancelClose` when a tray icon exists. Escape also hides when tray exists; without a tray, Escape minimizes. Explicit Quit via Ctrl+Q, tray menu or IPC bypasses close interception and exits. Number/Space shortcuts do not intercept text entry while egui reports keyboard input is wanted.
- Isolated GUI smoke on Nobara: private D-Bus session, Xvfb, temporary HOME/XDG configuration/data/runtime, `PADFIRE_DISABLE_HARDWARE=1`. The release binary started, `--status` returned a valid stopped/no-hardware JSON snapshot, `--show`, `--reload`, `--stop`, `--quit` each returned `ok`, app exited cleanly, IPC socket was removed, log was empty. This does not replace visible KDE Wayland/Windows tray acceptance.
- LUFS: FFmpeg loudnorm on the first 120 seconds, -18 LUFS target, true-peak report, 1-150% suggested gain, explicit Apply action, cancellation and timeout.
- Playback/output meter: the primary app-mix fallback uses 256-sample local accumulation, one packed `AtomicU64` update per chunk, accounts for pad volume, aggregates concurrent Padfire sources and returns to zero while idle. A separate lazy system-output loopback worker runs only while Padfire audio is active: PulseAudio/PipeWire monitor source on Linux and WASAPI loopback on Windows. The callback reduces samples to RMS only; it does not retain or write audio. The app-mix meter is used whenever native loopback is unavailable; the worker is stopped while idle. UI smoothing is 35 ms attack / 240 ms release.
- Windows IPC: local named pipe, first-instance ownership, inherited user-token DACL, remote-client rejection, bounded newline framing, response ACK, shutdown/name-release and duplicate-instance tests.
- Windows config storage: config directory receives a protected DACL at `CreateDirectoryW` creation and is re-hardened through stable no-follow handles. The DACL grants full access to the owner, SYSTEM and local Administrators, blocks inherited broad principals, and propagates owner-only ACEs to children. Existing config files are re-locked through their open handles after owner, reparse-point and hardlink checks. Tests inspect real SDDL for the directory and config file, reject Everyone/Authenticated Users/Builtin Users, reject final-component junctions, and reject a junction in the immediate parent. Windows same-directory writes flush temp contents then replace via `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`; native tests verify previous-generation backup, DACL retention, sharing-violation preservation and temp cleanup. Ancestors above the immediate parent remain a trusted OS/user-profile path boundary.
- Windows process supervision: scripts and Crosshair commands launch with `CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, are assigned to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then have their initial thread resumed via Toolhelp32 enumeration and `ResumeThread`. Script generation is rechecked while the child remains suspended. Native regression starts a PowerShell parent and child, terminates the job, and waits for the descendant process handle to become signaled.
- Process actor: Started is sent before output reader threads can emit Output. `ManagedProcess` owns stdout/stderr reader `JoinHandle`s; all stop, timeout, natural exit and shutdown paths terminate the process tree and join readers before sending final lifecycle events. Native Windows actor tests cover start/output/Stop, simultaneous scripts/Stop All, engine shutdown and canceled-generation suppression.
- Side-by-side installer: Linux user-local binary, desktop file and SVG icon; Windows per-user binary and Start Menu shortcut. Integration is created before the executable is published. If integration or binary copy fails, newly created integration artifacts are rolled back. Linux desktop `Exec` quotes spaces and escapes literal `%` field codes, backslashes, quotes and `$`. No autostart or service; never touches v2.4.0 or user configuration.
- Windows deferred uninstall: if the running executable is inside the candidate install root, a hidden cleanup helper uses null stdio and `CREATE_NO_WINDOW`, retries removal while the executable is locked, and preserves sibling user configuration. Native regression holds a real deny-delete file handle, observes a cleanup retry, releases the lock and verifies removal.

## Current candidate: c53c3ea

The active candidate is c53c3ea757f01456e80ca0a13220929e3bebe4f4, PR #22. It integrates the reviewed Launchpad UI commits, the security hardening, cargo-deny dependency policy, and a descriptor-relative Unix PID-file opener that removes the previous direct user-controlled path open.

Local Nobara validation:
- Formatting: pass.
- Workspace tests: 144 passed, 0 failed (31 app, 2 CLI, 28 core, 23 compatibility, 60 platform).
- Strict Clippy with warnings denied: pass.
- Optimized release build: pass.
- cargo-deny 0.22.0: advisories, bans, licenses and sources pass.
- Focused PID-path regression suite: 9 passed, including symlinked parent rejection.
- Legacy GTK/Python: 83 core tests and 7 dialog tests pass; Python compile, shell syntax, ShellCheck and desktop-file validation pass.
- Isolated Xvfb grid and Settings review passed. c53 captures (540x480) are saved under /home/roaring/Workspace/PadfireReview-2026-11-23/c53c3ea/.

Unix PID-file opening now starts at the fixed filesystem root and walks each parent component using an open directory descriptor and openat with O_NOFOLLOW. The final component is opened relative to that verified parent descriptor with O_NOFOLLOW and O_NONBLOCK. Metadata checks remain on the opened handle. The configured path stays user-selectable; valid custom paths are preserved. The current autoclick configuration uses /run/user/1000/autoclicker/autoclick.pid, matching the live KDE runtime and /home/roaring/autoclick.sh.

### Third-party dependency notice - newly integrated installer work

A generated `rust/THIRD_PARTY_LICENSES.html` now records all locked Rust workspace runtime, build, development and transitive dependencies for Linux x86-64 and Windows x86-64. It contains 82 full license-text sections and 311 crate attribution links (296 distinct crate/version labels), generated offline from `Cargo.lock` by cargo-about 0.9.3 using `rust/about.toml` and `rust/about.hbs` with `--fail`. The report is 465 KB. CI installs pinned cargo-about 0.9.3, fetches the locked target graph, regenerates the report offline, and compares it byte-for-byte.

The installer now embeds that HTML and manages it with the executable/integration lifecycle: Linux location `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html`; Windows location beside `padfire-rust.exe`. Existing notice content is backed up during upgrades and restored on failure. Standard uninstall removes the notice; deferred Windows self-uninstall retries notice deletion after the running image unlocks it. Tests verify install/reinstall payload, failed-upgrade restoration, uninstall, and deferred Windows cleanup. The currently installed Nobara c53 binary predates this notice change; do not claim the new notice is installed until the new cross-platform CI passes and the refreshed binary is installed side-by-side.

The installer source-file boundary was also refactored after the `8c625a3` PR scan reported two path-injection flows at the legacy `source.is_file()` and `File::open(source)` callsites. Production Linux now opens the running process image through fixed `/proc/self/exe`, checks metadata on that open handle, and copies through a cloned file handle. The source path remains solely for the same-file short-circuit. The subsequent `77b40fe` CodeQL scan reduced the two alerts to one at a `#[cfg(test)]` path-opening adapter; that adapter and the corresponding copy adapter have now been removed entirely. Installer tests instead use `open_running_executable()` to obtain an already-open process-image handle. The focused installer suite passes 15/15, and the full 144-test/Clippy/release/cargo-deny/report-reproduction gate passes locally. A fresh CodeQL result for this final adapter-free tree is pending; do not treat the path findings as resolved until the changed-file policy is green.

The hosted Windows matrix exposed a real retry issue after switching executable copying to an open handle: cloned handles share their file cursor on Windows, so the second copy could start at EOF after a failed publish. `copy_executable` now explicitly seeks each cloned handle to offset zero before every copy attempt. A new cross-platform regression forces a failed publish, retries with the same handle, and verifies a complete executable plus no leaked temporary files. The focused installer suite passes 16/16; the full workspace is now 144 tests.

Hosted c53 validation:
- PR Rust, Ubuntu and Windows, UI render, cargo-deny and GTK smoke: all passed.
- Push Rust, Ubuntu and Windows, UI render and cargo-deny: all passed.
- Explicit PR CodeQL Rust, Python and Actions analyses: all passed.
- GitHub CodeQL policy check 138094831040: success, “No new alerts in code changed by this pull request.” The old #185 instance was not dismissed; the current diff no longer introduces it.

The separate main Code Scanning baseline remains 104 open alerts on commit 1cc3af4 (100 high path injection, 3 critical command-line injection, 1 critical unsafe deserialization). Issue #21 remains open for individual baseline triage.

## Nobara side-by-side installation: c53

Current install: /home/roaring/.local/opt/padfire-rust/padfire-rust
Version: Padfire 3.0.0-alpha.1
SHA-256: aef9169b9ff23e96091c14f7d36930a03b53329afcbd984675e0764e9343ba5b (exactly matches rust/target/release/padfire).
Desktop entry and SVG icon validate; KDE cache was refreshed. An unrelated pre-existing malformed packettracer.desktop emits a cache-refresh warning; the Padfire Rust entry itself validates.

Rollback binary/UI sets, each with verified SHA-256 manifests:
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-url-2.5.9/
- ~/.local/share/padfire-rust-backups/2026-11-23-pre-c53-openat/

The pre-change candidate configs are backed up under ~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/ with a verified manifest. Rust runtime config is ~/.config/padfire-rust/padfire.json (v5, 8 pages, 21 assignments; SHA-256 5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60). Source/default config is ~/.config/padfire/padfire.json (SHA-256 a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0). Both set the clicker PID path to /run/user/1000/autoclicker/autoclick.pid. Legacy v2.4.0 config is ~/.config/roaring/padfire.json (SHA-256 50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5); its desktop entry and launcher are unchanged.

Installer lifecycle was tested in a disposable HOME/XDG environment: first install, repeat install, launcher/icon validation, stopped status, uninstall, and byte-identical preservation of an existing v5 config all passed. The installed candidate remains stopped; no Rust process or stale IPC socket exists.
The notice-bearing 77b40fe Linux release is built locally at `rust/target/release/padfire`, SHA-256 `948a0cb618e11bd7e9c6bfd09b9989bc96dece14dfee45cdcee09ca9c16e39f3`. It passed a disposable HOME/XDG lifecycle after the handle refactor: install placed a byte-identical `THIRD_PARTY_LICENSES.html` beside the executable, repeat install preserved the report hash, normal uninstall removed the app-owned files, and a second uninstall with an added user note removed only Padfire files while retaining that note (and correctly reported that the non-empty install root was preserved). An existing v5 config in the disposable profile remained byte-identical. The real Nobara c53 install is still untouched until latest hosted CodeQL and both platform matrices pass.

The actual installed Padfire Rust desktop entry was also launched with gtk-launch in a private HOME/XDG, D-Bus and Xvfb environment with hardware disabled. Status returned the expected c53 version, --quit exited cleanly and the IPC socket was removed. This exercised the desktop-file Exec/Name/icon path without opening live MIDI or audio devices.

A private runtime smoke then loaded a copy of the actual Nobara Rust v5 config, redirected the copy’s PID path to a temp file, and exercised status/reload/show/quit under hardware-disabled Xvfb with no Pulse server. Status preserved page 0, monitor enabled and the pulseaudio:vm_music target. The real config hash remained unchanged and the private IPC socket was removed.

Legacy startup entries were verified read-only and left untouched: ~/.config/autostart/padfire.desktop and ~/.config/systemd/user/padfire.service both predate this Rust install (October 31) and target the legacy padfire-launch v2.4.0 binary. The user systemd unit is enabled but currently inactive; the legacy launcher reports v2.4.0 not running. The Rust installer created no autostart entry or service.

The live Launchpad Mini and LPD8 are attached. An aseqdump monitor is already connected to the LPD8, Carla is running a saved project, and PipeWire has active sinks/sources. No MIDI port, Carla session, PipeWire route, Scarlett/Astro device or physical controller was opened or changed. Physical acceptance remains a controlled user-visible step.

## 2026-11-22 follow-up audit

The local Rust worktree contains a selected-output loopback meter follow-up and
pins both third-party CI actions to verified immutable commits:
- `dtolnay/rust-toolchain` is pinned to
  `df2a88e9fe4098bad09cb42c3dec7187f15861fc`, with `toolchain: stable`
  stated explicitly.
- `Swatinem/rust-cache` is pinned to
  `d1527c8f67ef9af357290475d4af4b0fab8f12e0` (v2.9.3).
- The local Nobara worktree passed `cargo fmt --all -- --check`,
  `cargo test --workspace --locked`, strict Clippy and an optimized release
  build after the loopback-meter changes.
- Hosted Windows CI had one transient failure in the deferred-uninstall
  locked-file retry test; rerunning that exact failed job passed. The test's
  initial wait was lengthened to tolerate slower hosted-runner PowerShell
  startup. Re-run the complete hosted matrix on the new commit before merge.
- Visible KDE Wayland/Windows tray acceptance and physical MK1/MK2, PipeWire
  and WASAPI acceptance remain manual gates. No live audio route, Launchpad,
  production launcher or user config was changed.

## Immediate engineering queue

1. Keep PR #22 open for review of the integrated candidate; c53 hosted build, UI, cargo-deny, GTK and explicit CodeQL checks pass, with no new CodeQL alerts in changed PR files.
2. Continue individual source-to-sink triage of the separate 104-alert main baseline. Do not bulk-dismiss findings; issue #21 remains open.
3. Review cargo-deny duplicate-version warnings and complete signing/release provenance checks.
4. Complete behavior parity against BEHAVIOR_MATRIX.md, including editor/settings, CLI, MIDI reconnect and audio routing/meter semantics.
5. Review config concurrency/failure semantics and Windows ancestor trust boundary; stress process supervision and installer rollback.
6. Complete visible KDE Wayland/Windows 11 UI/tray/focus acceptance and physical Launchpad Mini MK1/MK2 MIDI/LED/unplug/reconnect, live PipeWire and WASAPI playback/monitor tests.
7. Complete v5 upgrade/rollback rehearsal and every CUTOVER.md gate before any production transition.

## Explicit non-goals until evidence gates pass

Do not replace v2.4.0, change its launcher, enable autostart, create an always-running service, modify live audio routing/config, or restart/claim the live Launchpad. Rust remains a parallel candidate, not production.
