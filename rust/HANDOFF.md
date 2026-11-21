# Rust rewrite handoff

Updated: 2026-11-20

## Repository checkpoint

- Repo: Roaring1/Padfire
- Base: main / tag v2.4.0 at
  f88d8fd54aa1a09e9c16474ecba89abe354e4bcf
- Work branch: rust/rewrite-foundation-20261120
- The released Linux app remains running and connected to the Launchpad.
  Do not stop/restart it or claim its MIDI ports during development.

## Baseline before edits

- python3 tests/test_padfire_core.py: 83 passed.
- xvfb-run -a python3 tests/test_padfire_dialogs.py: 7 passed.
- Live read-only status: connected=true, page=0, playing=[], version=2.4.0,
  monitor enabled.
- Rust compiler: 1.96.0; Cargo: 1.96.0; rustup command absent.
- Windows laptop was reachable once using ssh laptop, reporting Windows 11
  64-bit and PowerShell 5.1. Later SSH hostname resolution failed; attempts
  to the known LAN neighbor addresses did not re-establish a session. Do not
  claim Windows install/toolchain status beyond the first read-only check.

## Work completed on this branch

- Created Cargo workspace and padfire-core crate.
- Added initial v5 config compatibility/migration model.
- Added MK1/MK2 MIDI packet decoding, page mapping and LED constants.
- Added 17 hardware-free Rust compatibility tests.
- Added architecture decisions, feature parity matrix, subsystem map, stack
  research, cutover/rollback gates and this handoff.

## Immediate next steps

1. Add typed action dispatcher, generation-token playback state machine and
   bounded IPC schema to padfire-core.
2. Add secure per-OS config store, named-pipe/Unix-socket IPC and CLI.
3. Add MIDI and audio adapters with mock backends, then lifecycle tests.
4. Prototype native egui GUI and tray on Linux; add CI build for Windows.
5. Regain Windows SSH access for toolchain, hardware and install inspection.
6. Complete the Windows build, package and visible hardware/audio acceptance.

## Safety rule

Do not install a Rust binary over the current Linux or Windows app until all
subsystems pass the gates in CUTOVER.md. Preserve the v2.4.0 binary/config
rollback path.
