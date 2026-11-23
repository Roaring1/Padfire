# Padfire Rust rewrite handoff

Updated: 2026-11-23 (America/Los_Angeles)

## Checkpoint

- Repository: `Roaring1/Padfire`
- Production reference: released Padfire v2.4.0 on `main`; current security branch is based on `1cc3af4ebc7483c29c23e1bd7f41932948a99510`.
- Active security branch: `security/triage-codeql-paths-2026-11-23`
- Latest combined security + UI + supply-chain commit: `c1a52322431f26d803660fb9d2de29a83821258d` (`security: fix URL advisory and enforce dependency policy`).
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

## Combined security + UI candidate: local validation and Nobara install

The combined candidate head `c1a52322431f26d803660fb9d2de29a83821258d` passed local Nobara/Linux validation:

| Gate | Result |
|---|---:|
| `cargo fmt --all -- --check` | pass |
| Workspace unit/integration tests | **140 passed** |
| Strict Clippy (`-D warnings`) | pass |
| Optimized workspace release build | pass |
| `git diff --check` | pass |
| Legacy GTK/Python core suite | 83 passed |
| Legacy GTK dialog suite under Xvfb | 7 passed |
| Isolated Rust Xvfb grid + Settings render | 2 distinct 540x480 PNGs; visually inspected |

Rust test groups: 30 app, 2 CLI, 28 core, 23 compatibility, 57 platform. The combined branch includes the four reviewed PR #20 UI/animation/CI commits on top of the security changes; the hardened config parser, config store, installer, IPC and process-manager changes remain present. `cargo-deny 0.22.0 check` passes advisories, licenses, bans and sources. It identified RUSTSEC-2026-0592 in `url 2.5.8`; this was fixed by upgrading to 2.5.9 and `form_urlencoded` 1.2.3. Local workspace path dependencies now specify exact `3.0.0-alpha.1` versions. The new `rust/deny.toml` policy is enforced by a pinned cargo-deny CI job; OS-specific duplicate crate versions remain warnings.

The candidate was installed side-by-side on Nobara at `~/.local/opt/padfire-rust/padfire-rust` using its `--install` CLI. It reports `Padfire 3.0.0-alpha.1`. The latest installed binary SHA-256 is `02f6c890ed582bd22c94336d3564413e6287ff2a856348bc95d25172ae44be98`, matching the local release artifact after upgrading `url` to 2.5.9. The `Padfire Rust` desktop entry and SVG icon were regenerated and validated; KDE desktop cache was refreshed. Previous candidate files are backed up under both `~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/` and `~/.local/share/padfire-rust-backups/2026-11-23-pre-url-2.5.9/`, each with verified SHA-256 manifest. The installed candidate was not launched in the live desktop.

Configuration namespaces: the Rust runtime uses `~/.config/padfire-rust/padfire.json` (v5, 8 pages, 21 assignments; current SHA-256 `5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60`). The source/default candidate config is `~/.config/padfire/padfire.json` (current SHA-256 `a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0`). Both now set the clicker LED PID path to `/run/user/1000/autoclicker/autoclick.pid`, matching the actual KDE `XDG_RUNTIME_DIR` and `/home/roaring/autoclick.sh`; the prior `/tmp/autoclick.pid` value did not match the script's state path. The autoclick script was not launched. Both pre-change config files are preserved under `~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/` with a verified SHA-256 manifest. The legacy v2.4.0 config at `~/.config/roaring/padfire.json` is separate and unchanged (SHA-256 `50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5`). The v2.4.0 desktop entry/launcher remain unchanged.

### Hosted validation

The combined UI/security head `137327d` passed both Ubuntu and Windows Rust matrices, hosted UI screenshot rendering, GTK legacy smoke and all three CodeQL language analyses. Its CodeQL policy check reported one high PID-file path alert (#185). The supply-chain follow-up `c1a5232` adds `cargo-deny` policy, upgrades `url` to 2.5.9 and pins internal workspace dependency versions. All hosted validation for exact head `c1a5232` is now complete:
- Rust Ubuntu/Windows PR matrix: [45277552562](https://github.com/Roaring1/Padfire/actions/runs/45277552562) passed formatting, tests, strict Clippy and optimized release builds on both platforms.
- Push Rust matrix: [45277537020](https://github.com/Roaring1/Padfire/actions/runs/45277537020) passed Ubuntu and Windows.
- Dependencies / cargo-deny: passed on both PR and push workflows.
- Linux UI review and screenshot artifact: passed on both workflows.
- GTK legacy smoke: [45277552484](https://github.com/Roaring1/Padfire/actions/runs/45277552484) passed.
- Explicit PR CodeQL analysis: [45277537921](https://github.com/Roaring1/Padfire/actions/runs/45277537921) completed successfully for Rust, Python and Actions.

The separate GitHub CodeQL policy check [138053461156](https://github.com/Roaring1/Padfire/runs/138053461156) **failed** because it found one new high-severity alert: #185 at `rust/apps/padfire/src/runtime.rs:261`, the user-selected PID status-file path. The alert instance is on PR head c1a5232 and has not been dismissed or fixed. Successful completion of the analyzer workflow does not override this failed security check.

### Explicit security/release boundaries

The GitHub CodeQL policy check on current PR head c1a5232 is red for one high-severity finding: alert #185 at `runtime.rs:261`, the user-selected read-only PID status-file open. The resolver rejects traversal, opens without following links/reparse points, validates the opened handle and parses bounded numeric PIDs used only for external-process LED state. Keep this finding open for human review; do not suppress it.

The repository Code Scanning API separately reports 104 open alerts on `main` at baseline `1cc3af4` (100 high path-injection, 3 critical command-line-injection, 1 critical unsafe-deserialization). Those are separate from the current PR-ref finding. No alerts were bulk-dismissed. The latest previous PR analysis removed the critical deserialization result and arbitrary `PADFIRE_INSTALL_ROOT` override path from the current PR alert set.

Local `cargo-deny check` 0.22.0 passes all four sections (advisories, bans, licenses, sources). It found and prompted the lockfile upgrade from vulnerable `url` 2.5.8 to fixed 2.5.9 (RUSTSEC-2026-0592); exact workspace versions were added to internal path dependencies. Duplicate versions remain warnings for transitive/target-specific crates, not build failures. The new `rust/deny.toml` and pinned cargo-deny CI job enforce the policy going forward. Hosted c1a5232 cargo-deny CI passed on both PR and push workflows.

Physical Launchpad Mini MK1/MK2 input/LED/reconnect, visible KDE Wayland/Windows tray/focus acceptance, live PipeWire/WASAPI playback/monitor, dependency/license/signing, v5 upgrade and rollback rehearsal remain outstanding. Production v2.4.0 and its audio/MIDI routing remain untouched.

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

1. Confirm exact c1a5232 hosted Rust/UI/GTK/CodeQL and the new cargo-deny policy job finish successfully; inspect failures before considering the combined branch hosted-verified.
2. Review current PR CodeQL alert #185 (user-selected PID status-file path) and its trust boundary explicitly. Do not suppress or dismiss the high finding merely to make the check green.
3. Continue individual source-to-sink triage of the separate 104-alert main baseline; update issue #21 only with evidence-backed dispositions.
4. Review duplicate dependency-version warnings from cargo-deny; retain `warn` until upstream compatibility is assessed rather than blocking on target-specific duplicates.
5. Complete behavior parity against `BEHAVIOR_MATRIX.md`, including full editor/settings, CLI, MIDI reconnect and audio routing/meter semantics.
6. Review config concurrency/failure semantics and Windows ancestor trust boundary; stress process supervision and installer rollback.
7. Complete visible KDE Wayland/Windows 11 UI/tray/focus acceptance and physical Launchpad Mini MK1/MK2 MIDI/LED/unplug/reconnect, live PipeWire and WASAPI playback/monitor tests.
8. Complete v5 upgrade/rollback rehearsal, signed/checksummed release artifacts and every `CUTOVER.md` gate before any production transition.

## Explicit non-goals until evidence gates pass

Do not replace v2.4.0, change its launcher, enable autostart, create an always-running service, modify live audio routing/config, or restart/claim the live Launchpad. Rust remains a parallel candidate, not production.
