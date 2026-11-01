# Roadmap

This roadmap distinguishes current behavior from ideas that are deliberately
not part of the current release.

## Current supported hardware

- Novation Launchpad Mini MK1 and MK2, using Padfire's existing note/CC and LED
  protocol.
- The Launchpad Mini MK3 is intentionally rejected. Its protocol differs; the
  MK1/MK2 LED messages must not be sent to it.
- Generic MIDI devices, other Launchpad models, keyboards, gamepads and
  arbitrary USB devices are **not currently supported**.

## Far future: device-neutral controller and input support

**Status: deferred exploration; no implementation commitment or ETA.**

The long-term direction is to let users connect more than the currently
supported Launchpad Mini models, with device profiles rather than hardcoded
device-name checks.

A safe progression would be:

1. **Generic MIDI input profiles.** Let users select an available MIDI input,
   inspect incoming note/CC/program-change events, and map those events to
   pages, pads and actions. Keep the existing Mini MK1/MK2 driver as a known
   good profile.
2. **Controller-specific output profiles.** Add LED/display feedback only
   when a device's protocol is understood and tested. Input support must not
   imply that Padfire can safely send arbitrary output messages.
3. **Common controller templates.** Evaluate other Launchpad generations,
   MIDI pad controllers, control surfaces and common macro-pad devices based
   on hardware access and protocol documentation.
4. **Selected non-MIDI USB inputs.** Explore standard OS-level interfaces for
   USB HID keyboards, consumer-control devices and gamepads. Require explicit
   device selection, visible event inspection and narrowly scoped permissions.

Padfire should not claim to support "any USB input" or open arbitrary USB
interfaces by default. Generic raw USB access can involve device-specific
protocols, elevated permissions, exclusive device claims and unsafe output
commands. Prefer standard MIDI/HID APIs, explicit user consent, declarative
profiles and capability-specific drivers. Never send LED, firmware or vendor
control messages to a device merely because its name looks similar.

Tracking issue: [#8 — device-neutral MIDI profiles and carefully scoped USB HID support](https://github.com/Roaring1/Padfire/issues/8).

## Related project

[ruohki/lunchpad](https://github.com/ruohki/lunchpad) is a separate Launchpad
macro application with support for a wider range of Novation controllers and
additional integrations. It is a useful related project to compare when
planning broader controller coverage. Padfire is independent and does not
reuse or depend on its code.
