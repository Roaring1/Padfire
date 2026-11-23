# Padfire Rust rewrite handoff

Updated: 2026-11-23 (America/Los_Angeles)

## Checkpoint

- Repository: `Roaring1/Padfire`
- Production reference: released Padfire v2.4.0 on `main`; current security branch is based on `1cc3af4ebc7483c29c23e1bd7f41932948a99510`.
- Active security branch: `security/triage-codeql-paths-2026-11-23`
- Latest security implementation commit: `cb0a168678d189185d70d9161d9558e3cb3d75ff` (`fix: make PowerShell installer temp files collision-safe`).
- Security remediation PR #22: https://github.com/Roaring1/Padfire/pull/22 (open, non-draft, unmerged).
- UI/final-build candidate PR #20: https://github.com/Roaring1/Padfire/pull/20 (open, draft, unmerged).
- Both are candidates only; production v2.4.0 remains the reference and no cutover is authorized by these PRs.
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

## Latest validation on commit cb0a168

Nobara/Linux and the hosted Windows runner both passed the complete Rust workspace validation on the exact security PR head.

| Gate | Nobara/Linux | Hosted Windows |
|---|---:|---:|
| cargo fmt --all -- --check | pass | pass |
| Workspace unit/integration tests | 129 passed | 133 passed |
| Strict Clippy (-D warnings) | pass | pass |
| Optimized workspace release build | pass | pass |

Linux test groups: 19 app, 2 CLI, 28 core, 23 compatibility and 57 platform. Windows test groups: 19 app, 2 CLI, 28 core, 23 compatibility and 61 platform. Windows-only additions cover exclusive-create collision ownership, two distinct generated PowerShell scripts executed through powershell.exe -File, and the production wrapper.

Hosted CI for head cb0a168678d189185d70d9161d9558e3cb3d75ff:
- Padfire tests run 45170215775 passed, including hardware-free GTK tests, icon loading and virtual-display rendering.
- Padfire Rust run 45170215759 passed formatting, tests, strict Clippy and optimized release builds on Ubuntu and Windows.
- PR CodeQL run 45170207634 completed successfully: Rust reported 1 high path-injection result; Python and Actions reported 0.

The separate PR CodeQL policy check is red because the one high finding remains open: alert #185 at runtime.rs:261, the user-selected PID status-file open. This is a documented read-only feature; the resolver rejects traversal, opens without following links/reparse points, validates the opened handle and parses bounded numeric PIDs used only for external-process LED state. Keep the finding open for human review; do not suppress it.

The repository Code Scanning API separately reports 104 open alerts on refs/heads/main at baseline 1cc3af4ebc7483c29c23e1bd7f41932948a99510 (100 high path-injection, 3 critical command-line-injection, 1 critical unsafe-deserialization). Those are a separate main-branch baseline, not the current PR-ref finding. No alerts were dismissed. The critical unsafe-deserialization and installer-root override findings are absent from the latest PR analysis.

No visible KDE Wayland or Windows 11 tray acceptance, physical MIDI hardware test, or live PipeWire/WASAPI routing test is claimed. Production v2.4.0 and its user configuration/audio routing were not changed.

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

1. Keep PR #22 open for human review of the one current high-severity PID-file path finding (#185 at runtime.rs:261); the PR CodeQL policy check remains red until that finding has a reviewed disposition.
2. Continue individual source-to-sink triage of the separate 104-alert main baseline. Do not bulk-dismiss findings; update issue #21 only with evidence-backed dispositions.
3. Add dependency/license policy and provenance checks; cargo-deny or equivalent is not currently configured.
4. Continue behavior parity against BEHAVIOR_MATRIX.md: full pad editor fields/settings, CLI JSON/exit behavior, MIDI reconnect, per-pad audio routing, monitor and meter semantics.
5. Review config backup/concurrency/failure semantics and Windows ancestor trust boundary; test crash/power-loss durability before production cutover.
6. Stress process supervision with high-volume simultaneous stdout/stderr, descendants, repeated start/stop, timeout and shutdown; inspect for orphan handles/readers.
7. Complete installer custom-integration restoration, exotic path matrix, signing and upgrade/rollback rehearsal.
8. Perform visible KDE Wayland and Windows 11 UI/tray/focus acceptance, then physical Launchpad Mini MK1/MK2 MIDI/LED/unplug/reconnect, PipeWire playback/monitor and WASAPI playback/monitor tests.
9. Complete every CUTOVER.md gate, produce checksummed release artifacts, and only then decide on an explicit reviewed production transition with v2.4.0 rollback retained.

## Explicit non-goals until evidence gates pass

Do not replace v2.4.0, change its launcher, enable autostart, create an always-running service, modify live audio routing/config, or restart/claim the live Launchpad. Rust remains a parallel candidate, not production.
