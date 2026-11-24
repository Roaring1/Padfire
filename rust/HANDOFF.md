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

## Current candidate: 49311c3

The integrated Rust candidate is `49311c3a80c71484fd86afd3edb9ee667cea8826`, PR #22 (open, non-draft, mergeable). It includes the selected Launchpad UI work, security hardening, the descriptor-relative PID-file opener, safe executable-source handle copying, dependency policy and the bundled third-party notice. It remains `3.0.0-alpha.1`, side-by-side with the legacy v2.4.0 release.

### Validation

Local Nobara validation on the exact 49311c3 tree:
- `cargo fmt --all -- --check`: pass.
- `cargo test --workspace --locked`: **144 passed, 0 failed** (31 app, 2 CLI, 28 core, 23 compatibility, 60 platform).
- Focused installer tests: **16 passed**, including failed-publish/same-handle retry and the Windows shared-cursor rewind regression.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo build --workspace --release --locked`: pass.
- `cargo deny check` 0.22.0: advisories, bans, licenses and sources pass.
- cargo-about 0.9.3: offline regeneration of `THIRD_PARTY_LICENSES.html` with `--fail`, byte-for-byte comparison: pass.
- Focused PID path tests (9), hardware-free MIDI tests (7), synthetic audio/UI interaction tests, legacy GTK/Python (83 core + 7 dialog), shell syntax, Python compile, ShellCheck and desktop-entry checks pass.

Hosted validation on exact 49311c3:
- Push Rust workflow [45320135973](https://github.com/Roaring1/Padfire/actions/runs/45320135973): Ubuntu and Windows format/test/strict Clippy/release, UI screenshot, cargo-deny and notice regeneration all passed.
- Pull-request Rust workflow [45320142859](https://github.com/Roaring1/Padfire/actions/runs/45320142859): Ubuntu and Windows matrices passed.
- PR workflow [45320137717](https://github.com/Roaring1/Padfire/actions/runs/45320137717): explicit CodeQL and PR policy passed.
- Explicit Rust CodeQL SARIF [2235814466], Python [2235797264] and Actions [2235796428]: **0 findings each**.
- GitHub changed-file policy check [138196612506](https://github.com/Roaring1/Padfire/runs/138196612506): success, “No new alerts in code changed by this pull request.”
- Current `refs/pull/22/head` Code Scanning inventory: **0 open alerts**.

The original high-severity PID-file path findings were resolved without dismissing alerts: Unix uses a root-anchored descriptor-relative `openat(O_NOFOLLOW)` walk for each parent component and final filename; installer executable copy uses the already-open process-image handle (`/proc/self/exe` on Linux), with no path-opening test adapters. On Windows, cloned file handles share their cursor, so the copy routine seeks every clone to byte zero before every retry; a regression forces a failed publish and verifies a same-handle retry copies the whole binary.

### Third-party notice

`rust/THIRD_PARTY_LICENSES.html` is a 465 KB deterministic HTML report for Linux x86-64 and Windows x86-64, all runtime/build/dev/transitive locked dependencies: 82 full license sections, 311 attribution entries and 296 distinct crate/version labels. It is generated by pinned cargo-about 0.9.3. CI regenerates it offline from the lockfile and byte-compares the checked-in file. The installer stores it beside the executable at `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html`; failed upgrades restore any previous notice, ordinary uninstall removes the managed file, and deferred Windows uninstall retries notice deletion after the executable unlocks.

### Nobara side-by-side installation: 49311c3

Installed path: `~/.local/opt/padfire-rust/padfire-rust`.
- Version: `Padfire 3.0.0-alpha.1`.
- Binary SHA-256: `e3543668e54ea335cedfd8d43afc84b594d71ed1f319ad3afb5b3aa155bdc6aa`, exactly matches `rust/target/release/padfire`.
- Bundled notice SHA-256: `34cc307a6726f1d6bc43d7de6aab3ec2eb9db7463fe8b21f0cbf52b61542d06b`, exactly matches `rust/THIRD_PARTY_LICENSES.html`.
- `--status`: `running:false`. No Rust process or IPC socket remains.
- Desktop entry and SVG icon validate; KDE cache refresh succeeded. It prints a warning for an unrelated pre-existing malformed `packettracer.desktop`; the Padfire Rust desktop entry itself validates.
- Native x86-64 ELF loader and all dynamic libraries/symbols resolve (`ldd`, `ldd -r`).
- Installed desktop entry launched successfully using `gtk-launch` in private D-Bus/Xvfb with hardware disabled; status and quit worked and the IPC socket was removed.
- A private runtime smoke loaded a copy of the actual v5 Rust config. It preserved page 0, monitor enabled and `pulseaudio:vm_music`; reload/show/quit passed. The real source config SHA-256 remains `5aada0eea0a6e80206a8f4f85343be1b6c3cb46ce6714830e12e4d72c7a82c60`.
- Source/default config remains v5 with SHA-256 `a003c9046235f987e2fd4ac907afa4d6eb55d23641aece42dd5da25fcb67edb0`; both candidate configs point to `/run/user/1000/autoclicker/autoclick.pid`.
- Legacy v2.4.0 config remains unchanged at SHA-256 `50f8e18175f88707649ace4febdab6492e8e553feb35103752a00334d22055c5`; legacy launcher and desktop entry are unchanged.
- Disposable install lifecycle on the exact final binary passed install/report byte equality/repeat install/clean uninstall and byte-identical v5 config preservation. An additional uninstall test verifies user-added files survive.

Rollback binaries/UI are retained with verified manifests:
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-combined-ui/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-url-2.5.9/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-c53-openat/`
- `~/.local/share/padfire-rust-backups/2026-11-23-pre-49311c3/` (c53 binary/desktop/icon)
- `~/.local/share/padfire-rust-backups/2026-11-23-pid-path-before-fix/` (pre-change config copies)

Legacy startup integrations remain untouched: the pre-existing Oct 31 `~/.config/autostart/padfire.desktop` and enabled-but-inactive `~/.config/systemd/user/padfire.service` both point to legacy `padfire-launch` v2.4.0. The Rust candidate created no autostart entry or service.

The live Launchpad Mini and LPD8 remain attached, an `aseqdump` monitor is connected to LPD8, Carla has a saved project running, and PipeWire is active. No MIDI port, Carla session, physical controller or live audio route was opened or changed. Physical Launchpad input/LED/unplug/reconnect, visible KDE Wayland/Windows 11 tray/focus, real PipeWire/WASAPI routing/playback/monitor/VU, dual-machine acceptance and signing/provenance remain production cutover gates. Keep v2.4.0 as production/rollback until these manual gates are accepted.

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
