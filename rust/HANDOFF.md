# Padfire Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: `Roaring1/Padfire`
- Production base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf` (released v2.4.0)
- Working branch: `rust/rewrite-foundation-20261120`
- Latest implementation commit: `b58f9566e656433d44c79bbdb70ec9d24ca09ee9`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- PR remains open, unmerged and draft; production v2.4.0 remains the reference.
- Nobara checkout: `/home/roaring/Documents/GitHub/Padfire`
- Windows checkout: `%USERPROFILE%\src\Padfire` on SSH alias `laptop`

Do not modify the production launcher, replace v2.4.0, change live audio routing/configuration, or claim physical MIDI hardware during unattended development.

## Current implementation

The Rust workspace contains `padfire-core`, `padfire-platform`, and a native eframe/egui desktop app. It includes v1-v5 config compatibility, generation-safe pad actions, MIDI protocol/reconnect, native audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, tray controls, cancellable FFmpeg LUFS analysis, playback RMS metering, and per-user side-by-side install/uninstall.

- Tray menu: Show, Stop Audio, Quit; code-generated icon; initialized on the UI thread.
- Window lifecycle: titlebar Close hides via `Visible(false)` with `CancelClose` when a tray icon exists. Escape also hides when tray exists; without a tray, Escape minimizes. Explicit Quit via Ctrl+Q, tray menu or IPC bypasses close interception and exits. Number/Space shortcuts do not intercept text entry while egui reports keyboard input is wanted.
- Isolated GUI smoke on Nobara: private D-Bus session, Xvfb, temporary HOME/XDG configuration/data/runtime, `PADFIRE_DISABLE_HARDWARE=1`. The release binary started, `--status` returned a valid stopped/no-hardware JSON snapshot, `--show`, `--reload`, `--stop`, `--quit` each returned `ok`, app exited cleanly, IPC socket was removed, log was empty. This does not replace visible KDE Wayland/Windows tray acceptance.
- LUFS: FFmpeg loudnorm on the first 120 seconds, -18 LUFS target, true-peak report, 1-150% suggested gain, explicit Apply action, cancellation and timeout.
- Playback meter: 256-sample local accumulation, one packed `AtomicU64` update per chunk, accounts for pad volume, aggregates concurrent Padfire sources, zero when idle, 35 ms attack / 240 ms release. It measures Padfire's mix, not unrelated system audio or post-device loopback.
- Windows IPC: local named pipe, first-instance ownership, inherited user-token DACL, remote-client rejection, bounded newline framing, response ACK, shutdown/name-release and duplicate-instance tests.
- Windows config storage: config directory receives a protected DACL at `CreateDirectoryW` creation and is re-hardened through stable no-follow handles. The DACL grants full access to the owner, SYSTEM and local Administrators, blocks inherited broad principals, and propagates owner-only ACEs to children. Existing config files are re-locked through their open handles after owner, reparse-point and hardlink checks. Tests inspect real SDDL for the directory and config file, reject Everyone/Authenticated Users/Builtin Users, reject final-component junctions, and reject a junction in the immediate parent. Windows same-directory writes flush temp contents then replace via `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`; native tests verify previous-generation backup, DACL retention, sharing-violation preservation and temp cleanup. Ancestors above the immediate parent remain a trusted OS/user-profile path boundary.
- Windows process supervision: scripts and Crosshair commands launch with `CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, are assigned to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then have their initial thread resumed via Toolhelp32 enumeration and `ResumeThread`. Script generation is rechecked while the child remains suspended. Native regression starts a PowerShell parent and child, terminates the job, and waits for the descendant process handle to become signaled.
- Process actor: Started is sent before output reader threads can emit Output. `ManagedProcess` owns stdout/stderr reader `JoinHandle`s; all stop, timeout, natural exit and shutdown paths terminate the process tree and join readers before sending final lifecycle events. Native Windows actor tests cover start/output/Stop, simultaneous scripts/Stop All, engine shutdown and canceled-generation suppression.
- Side-by-side installer: Linux user-local binary, desktop file and SVG icon; Windows per-user binary and Start Menu shortcut. Integration is created before the executable is published. If integration or binary copy fails, newly created integration artifacts are rolled back. Linux desktop `Exec` quotes spaces and escapes literal `%` field codes, backslashes, quotes and `$`. No autostart or service; never touches v2.4.0 or user configuration.
- Windows deferred uninstall: if the running executable is inside the candidate install root, a hidden cleanup helper uses null stdio and `CREATE_NO_WINDOW`, retries removal while the executable is locked, and preserves sibling user configuration. Native regression holds a real deny-delete file handle, observes a cleanup retry, releases the lock and verifies removal.

## Latest full local validation on commit `b58f956`

Both actual machines point to the same clean commit and passed the complete workspace gates:

| Gate | Nobara Linux | Windows laptop |
|---|---:|---:|
| `cargo fmt --all -- --check` | pass | pass |
| Workspace unit/integration tests | 82 passed | 87 passed |
| Strict Clippy (`-D warnings`) | pass | pass |
| Optimized workspace release build | pass | pass |

Linux test groups: 2 app, 28 core, 17 compatibility, 35 platform. Windows: 2 app, 28 core, 17 compatibility, 40 platform. Windows platform tests include native named-pipe, hardlink, Start Menu COM shortcut, locked-file deferred uninstall, protected DACL inspection, final and parent junction rejection, MoveFileExW replacement/backup/failure recovery, Job Object descendant termination, ProcessEngine start/output/Stop, Stop All/shutdown, and canceled-generation suppression.

The UI change on this commit passed the full matrix. Native app compilation passed on Windows; visible Windows tray interaction has not yet been accepted. Nobara isolated GUI smoke passed as detailed above. An earlier FFmpeg loudnorm end-to-end test with a generated sine WAV passed. MIDI hardware was disabled during GUI smoke.

Hosted GitHub Actions for `b58f956`: `Padfire tests` run `44857110249` completed successfully. `Padfire Rust` run `44857110283` was still in progress at the last check, with `Core / ubuntu-latest` and `Core / windows-latest` running. Recheck final results before marking hosted CI green.

## Immediate engineering queue

1. Verify hosted Actions `44857110283` pass both platforms; inspect logs if either fails.
2. Continue behavior parity against `BEHAVIOR_MATRIX.md`, especially the full pad editor fields, settings, CLI JSON/exit behavior, MIDI reconnect lifecycle, and audio output/monitor semantics.
3. Review config concurrency and failure semantics: backup-write failure, simultaneous saves, Windows parent-path trust, reparse attacks above the immediate parent, crash/power-loss durability. Add a serialization primitive only if multiple writers are possible in actual call paths.
4. Stress process output reader ownership with high-volume simultaneous stdout/stderr, long-running descendants, repeated start/stop, timeout and shutdown; verify bounded events and no orphan handles/threads.
5. Test installer rollback when executable copy fails after integration creation; preserve pre-existing custom integration during failed upgrade; test percent, apostrophe, quotes, backslashes and Unicode paths on both platforms.
6. Visible desktop acceptance on KDE Wayland and Windows 11: resize, HiDPI, tray menu, close-to-tray, Escape hide, restore/focus and resource behavior.
7. Physical Mini MK1/MK2 MIDI/LED/unplug/reconnect acceptance and PipeWire/WASAPI playback/monitor acceptance when it can be done without disrupting production.
8. Add dependency/license/security gates, CodeQL or equivalent, packaging, checksums/signatures, upgrade/rollback rehearsal.
9. Only after every `CUTOVER.md` gate passes, plan deliberate production cutover with legacy rollback retained.

## Explicit non-goals until evidence gates pass

Do not replace v2.4.0, change its launcher, enable autostart, create an always-running service, modify live audio routing/config, or restart/claim the live Launchpad. Rust remains a parallel candidate, not production.
