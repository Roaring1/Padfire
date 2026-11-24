# Rust cutover and rollback gates

**Current decision: no production cutover.** The Rust workspace is being built
alongside the released app. Do not replace the Linux launcher, Windows
installer, desktop entry, user service, config or running processes until all
gates below are checked.

## Development safety

- Keep main at v2.4.0 until the Rust candidate has meaningful review and CI.
- Do not stop, restart, reconfigure or claim the live Launchpad during tests.
- Use temporary HOME/APPDATA locations for config and IPC tests.
- Physical MIDI and audio tests are manual, scheduled only when the user can
  verify the actual controller, routed outputs and monitor sound.
- Preserve the old executable and v5 config backup before installing a
  candidate. Never delete or overwrite the legacy app during Rust development.

## Required gates

1. **Behavior:** every item in BEHAVIOR_MATRIX.md has a deterministic test or
   an explicitly recorded manual acceptance.
2. **Config safety:** both OSs read the existing v5 schema and can roll back to
   v2.4.0 without losing settings. Atomic write, backup, size and ownership
   rules are verified.
3. **Device safety:** MK3 negative tests pass; MK1/MK2 input, LED states,
   disconnect and reconnect are physically verified.
4. **Audio:** simultaneous sound playback, volume, stop races, per-pad routes,
   default routing, loop/retrigger, second-output monitor and VU are physically
   verified independently on Nobara and Windows.
5. **UI:** all editor/settings/tray/window/keyboard/drop interactions pass on
   KDE Wayland and Windows 11; no focus theft or unwanted autostart.
6. **Reliability:** close/reopen, sleep/resume, USB reconnect, audio-server
   restart, device removal and repeated start/stop have no leaked children,
   threads, streams, stale IPC files or duplicate instances.
7. **Build/security:** clean Linux and Windows matrix passes formatting,
   clippy, unit/integration tests, dependency and license checks, and CodeQL.
8. **Release:** exact candidate commit is merged to main, a versioned release
   is published, and matching build artifacts/checksums are attached.

## Install sequence after evidence gates

1. Build verified candidate artifacts and record SHA-256.
2. Back up existing config and install tree without removing the old binary.
3. Install Rust binary side-by-side under a versioned path.
4. Run --version, --status, config read-only validation and UI smoke test.
5. Start the Rust app only when the user is ready to verify MIDI/audio.
6. Verify all physical acceptance and IPC/CLI commands.
7. Change the launcher/desktop entry only after physical acceptance.
8. Retain v2.4.0 rollback binary and config backup through at least one
   successful restart and device reconnect.

## Rollback

Stop only the Rust process using its own PID/IPC identity, restore the legacy
launcher/desktop entry, and launch v2.4.0 against the unchanged v5 config.
Never use a broad process-name kill or delete a socket unless its inode/type
and owner have been verified.


## Third-party license notices

- [x] Regenerate `rust/THIRD_PARTY_LICENSES.html` from the exact locked workspace using cargo-about 0.9.3 and verify the checked-in report with the dependency-policy CI byte-comparison step.
- [x] Confirm Linux install places the report at `~/.local/opt/padfire-rust/THIRD_PARTY_LICENSES.html`; Windows target path and install logic are covered by hosted tests.
- [x] Verify upgrade rollback restores a pre-existing notice, normal uninstall removes only the managed notice, and deferred Windows uninstall removes it after the executable unlocks.
- [ ] Include the notice in signed/checksummed release artifacts and complete release provenance/signing. The report is already bundled in the side-by-side alpha and cargo-deny validates all license expressions.
