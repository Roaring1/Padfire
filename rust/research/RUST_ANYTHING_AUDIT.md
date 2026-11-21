# rust-anything whole-app audit

Date: 2026-11-20. Tool: official npm distribution, rust-anything 2.1.0.
The binary was installed under /tmp/padfire-rust-anything; no setup command
was run and no editor/agent settings were changed.

## Tool output

The whole-app analyzer reported:

- One Python source file (padfire.py), roughly 2,800 lines.
- Three Python test files.
- The gi and mido dependencies.
- Concurrency hotspots, dynamic typing, exception-heavy boundaries,
  string slicing, unordered map iteration and integer/division semantics.
- The whole-app detector classified the process as a server because it found
  a .listen() call associated with local IPC.
- It proposed researching gi and mido replacements before porting.

## What is useful

- Concurrency and dynamic typing are genuine migration risks. Padfire has
  long-lived audio, MIDI, VU, UI, script and IPC lifecycles, so line-by-line
  translation would be an unsafe approach.
- mido belongs behind a MIDI adapter and a shared typed message model.
- gi is not just a package replacement question: the GTK application,
  tray, event loop, keyboard handling and pad editor are one UI subsystem.
- Behavior must be recorded and replayed before implementation switches.

## Corrections from the source audit

The server classification is a false positive. The Python program is a native
GTK desktop GUI with a local Unix-domain control socket; its socket listener
does not make its product architecture a server. Do not create an Axum server
front door for the Rust application.

The analyzer sees only two direct Python imports and misses important runtime
contracts implemented via subprocesses and OS APIs:

- paplay, pactl and parec implement Linux playback, endpoint discovery
  and monitor/VU capture over PipeWire's PulseAudio compatibility layer.
- Windows uses a different WASAPI/PortAudio endpoint model in the draft port.
- Launchpad MK1/MK2 MIDI is a narrow device protocol, not generic MIDI input.
- Tray and native desktop lifecycle behavior are essential even though the
  tool reports zero UI files.
- Private config file handling, atomic backups, one-instance IPC, script
  process groups and Crosshair CLI launching are first-class security and
  lifecycle behaviors.

The whole-app report is therefore an inventory hint, not a complete
requirements specification. See BEHAVIOR_MATRIX.md for the inspected
behavior contract and STACK.md for researched crate candidates.
