# Mido replacement research

mido is the only direct Python MIDI dependency in the current GTK application.
The supported hardware boundary is intentionally narrow: Novation Launchpad
Mini MK1/MK2 only. MK3 is excluded because it uses a different protocol.
Padfire needs MIDI input, LED output, port discovery, disconnect/reconnect,
and serialized LED updates.

## Decision

Use midir behind a Padfire-owned adapter and reconnect state machine.

The shared padfire-core::midi module already owns protocol decoding,
page mapping and LED encoding. The OS adapter must not reimplement those
semantics. It enumerates input and output endpoints separately, matches the
supported device profile conservatively, opens the output side only after
confirming the profile, and exposes explicit unavailable/disconnected states.
It must never send a guessed LED message to an unknown controller.

midir documents ALSA input on Linux and WinMM input on Windows, with optional
WinRT support on Windows. The target adapter must confirm input and output
are both supported by the backend and that the same device identity is paired
safely. Port names alone are not permanent device IDs.

## Required tests

- NoteOn, NoteOff and velocity-zero NoteOn.
- CC104-111 page selection, including value zero ignored.
- Correct 8x8 and side-column mapping.
- Exact LED bytes and double-buffer flags.
- MK3 rejection before output port open.
- Partial input/output open failure cleanup.
- Port disappearance while connected.
- Reconnect without reopening the GUI.
- Stop/quit joins callbacks and prevents stale reconnect work.
- No real hardware is opened by unit tests.

References:
- https://docs.rs/midir/latest/midir/
- https://github.com/Boddlnagg/midir
