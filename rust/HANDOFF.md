# Padfire Rust rewrite handoff

Updated: 2026-11-20 (America/Los_Angeles)

## Checkpoint

- Repository: `Roaring1/Padfire`
- Production base: `main` at `f88d8fd54aa1a09e9c16474ecba89abe354e4bcf` (released v2.4.0)
- Working branch: `rust/rewrite-foundation-20261120`
- Latest implementation commit: `835501ff936ed9c2a0cdaa7e03a135cff9e0f01c`
- Draft PR: https://github.com/Roaring1/Padfire/pull/19
- PR remains open, unmerged and draft; production v2.4.0 remains the reference.
- Nobara checkout: `/home/roaring/Documents/GitHub/Padfire`
- Windows checkout: `%USERPROFILE%\src\Padfire` on SSH alias `laptop`

Do not modify the production launcher, replace v2.4.0, change live audio routing/configuration, or claim physical MIDI hardware during unattended development.

## Current implementation

The Rust workspace contains `padfire-core`, `padfire-platform`, and a native eframe/egui desktop app. It includes v1-v5 config compatibility, generation-safe pad actions, MIDI protocol/reconnect, native audio playback/routing, script process supervision, Linux Unix-socket IPC, Windows named-pipe IPC, tray controls, cancellable FFmpeg LUFS analysis, playback RMS metering, and per-user side-by-side install/uninstall.

- Tray menu: Show, Stop Audio, Quit; code-generated icon; initialized on the UI thread.
- LUFS: FFmpeg loudnorm on the first 120 seconds, -18 LUFS target, true-peak report, 1-150% suggested gain, explicit Apply action, cancellation and timeout.
- Playback meter: 256-sample local accumulation, one packed `AtomicU64` update per chunk, accounts for pad volume, aggregates concurrent Padfire sources, zero when idle, 35 ms attack / 240 ms release. It measures Padfire's mix, not unrelated system audio or post-device loopback.
- Windows IPC: local named pipe, first-instance ownership, inherited user-token DACL, remote-client rejection, bounded newline framing, response ACK, shutdown/name-release and duplicate-instance tests.
- Windows config storage: config directory receives a protected DACL at `CreateDirectoryW` creation and is re-hardened through a stable directory handle when it already exists. The DACL grants full access to the owner, SYSTEM and local Administrators, blocks inherited broad principals, and propagates owner-only ACEs to children. Existing config files are re-locked through their open handles after owner, reparse-point and hardlink checks. Tests inspect real SDDL for the directory and config file, reject Everyone/Authenticated Users/Builtin Users, and reject an NTFS directory junction. Atomic writes flush temp file contents and replace with same-directory `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`; tests verify previous-generation backup and DACL retention, and simulate a sharing-violation by holding the destination without delete sharing. The failed replacement preserves the old config and removes its temporary file. The final config-directory component is protected; ancestors such as the user-provided AppData parent remain a trusted path boundary.
- Windows process supervision: scripts and Crosshair commands launch with `CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, are assigned to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then have their initial thread resumed via Toolhelp32 enumeration and `ResumeThread`. Script generation is checked again while the child remains suspended, preventing stale/cancelled work from executing. This closes the spawn/assignment race where a fast script could create descendants before Job Object assignment. Native regression starts a PowerShell parent and child, terminates the job, and waits for the descendant process handle to become signaled.
- Process actor: Started is sent before output reader threads can emit Output, so a fast command cannot present output before the UI receives its started state. `ManagedProcess` owns stdout/stderr reader `JoinHandle`s; all stop, timeout, natural exit and shutdown paths terminate the process tree and join readers before sending final lifecycle events. Native Windows actor tests cover start/output/Stop, simultaneous scripts/Stop All, engine shutdown, and canceled-generation suppression.
- Side-by-side installer: Linux user-local binary, desktop file and SVG icon; Windows per-user binary and Start Menu shortcut. Integration is created before the executable is published. If integration or binary copy fails, only newly created integration artifacts are rolled back and a newly created empty install root is removed. Linux desktop `Exec` quotes spaces and escapes literal `%` field codes, backslashes, quotes and `$`. No autostart or service. It does not touch v2.4.0 or user configuration.
- Windows deferred uninstall: if the running executable is inside the candidate install root, the app removes its Start Menu shortcut and launches a hidden cleanup helper. The helper uses explicit null stdio handles and `CREATE_NO_WINDOW`, retries directory removal while the executable is locked, and preserves sibling user configuration. Native regression holds a real deny-delete file handle, observes a cleanup retry, releases the lock, and verifies removal.

## Latest full local validation on commit `835501f`

Both actual machines passed the complete workspace gates:

| Gate | Nobara Linux | Windows laptop |
|---|---:|---:|
| `cargo fmt --all -- --check` | pass | pass |
| Workspace unit/integration tests | 82 passed | 86 passed |
| Strict Clippy (`-D warnings`) | pass | pass |
| Optimized workspace release build | pass | pass |

Linux test groups: 2 app, 28 core, 17 compatibility, 35 platform. Windows: 2 app, 28 core, 17 compatibility, 39 platform. Windows platform tests include native named-pipe, NTFS hardlink, Start Menu COM shortcut, locked-file deferred uninstall, DACL inspection, junction rejection, MoveFileExW replacement/backup/failure recovery, Job Object descendant termination, ProcessEngine start/output/Stop, Stop All/shutdown, and canceled-generation suppression. Linux platform tests include Unix socket, symlink/hardlink safety, integration rollback, desktop percent escaping, installer and FFmpeg. Process output-reader ownership changes are included in this validation.

The Windows Job Object change is especially important: assigning after a normal `Command::spawn()` permits the first instructions of a fast child to run before it is assigned. The app now creates it suspended, assigns it to the job, checks the script's generation again, then resumes the initial thread. The actual native descendant and actor lifecycle tests pass. `ResumeThread` locates the initial suspended thread using `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`; the child cannot create another thread or spawn a descendant while its initial thread remains suspended.

Windows config storage uses protected owner/SYSTEM/Administrators DACL (`OWNER RIGHTS`, `SYSTEM`, `BUILTIN\Administrators`) with object/container inheritance. It is assigned when creating the directory through `SECURITY_ATTRIBUTES`. Existing directories and config files are checked using no-follow handles, owner verification accepts the token user or token default owner (for elevated processes), reparse points and hardlinks are rejected, and `SetSecurityInfo` applies the protected DACL through the opened handle. Focused native tests inspect real SDDL and reject directory junctions. Atomic replacement is via `MoveFileExW` with replace-existing/write-through flags after `sync_all` on the temp file. This guarantees the move/write-through operation has completed before returning; it is not claimed as a Windows directory-handle `fsync` equivalent.

Hosted GitHub Actions on commit `835501f`: `Padfire tests` run `44856605040` completed successfully. `Padfire Rust` run `44856605024` was still in progress at the last check, with both `Core / ubuntu-latest` and `Core / windows-latest` running. Check the final result before calling hosted CI green. Full local Linux/Windows validation is independently complete.

Earlier isolated Linux GUI smoke passed with Xvfb and private D-Bus: tray registers, `--status`, `--show`, `--reload`, `--stop`, and `--quit` respond, app exits, and IPC socket cleanup succeeds. MIDI hardware was explicitly disabled. An FFmpeg loudnorm end-to-end test with a generated sine WAV passed.

## Immediate engineering queue

1. Confirm hosted Actions `44856605024` passes both platforms; inspect logs if not.
2. Harden config path trust: examine intermediate ancestors leading to AppData, reparse-point behavior, backup consistency, concurrent saves and replacement races. Keep trusted user profile path assumptions explicit.
3. Add config-store failure tests for backup-write failure and concurrent writers; make concurrent save behavior deterministic or explicitly serialize it if the app can save from multiple threads.
4. Review process actor timeout, shutdown and output-reader joining under high output, descendant output and repeated start/stop stress. Verify cancellation after child spawn, after job assignment and before resume.
5. Test installer rollback when binary copy fails after integration creation; preserve existing/custom desktop integration on failed candidate upgrade; test paths with percent, apostrophe, quotes, backslashes and Unicode on both platforms.
6. Continue full v2.4.0 behavior parity against `BEHAVIOR_MATRIX.md`, especially CLI JSON, keyboard/focus, close/hide, editor fields, routing and script behavior.
7. Verify the UI visibly on KDE Wayland and Windows 11, including resize/HiDPI/tray/focus and resource behavior.
8. Complete physical Mini MK1/MK2 MIDI/LED/unplug/reconnect acceptance and PipeWire/WASAPI playback/monitor acceptance when it can be done without disrupting production.
9. Add dependency/license/security gates, CodeQL or equivalent, packaging, checksums/signatures, upgrade/rollback rehearsal.
10. Only after the `CUTOVER.md` gates pass, plan a deliberate production cutover with legacy rollback retained.

## Explicit non-goals until evidence gates pass

Do not replace v2.4.0, change its launcher, enable autostart, create an always-running service, modify live audio routing/config, or restart/claim the live Launchpad. Rust remains a parallel candidate, not production.
