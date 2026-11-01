# Contributing

## Scope

Padfire is currently a Linux GTK3 desktop app for Launchpad Mini MK1/MK2.
Changes to MIDI input, LED output, audio playback, process lifecycle, config
migration and local IPC should preserve the existing supported hardware and
single-user trust model.

Do not assume other Launchpad generations share the MK1/MK2 protocol. Add a
separate, tested device profile before expanding hardware support.

## Local checks

Run the hardware-free core and UI checks:

```bash
python3 tests/test_padfire_core.py
xvfb-run -a python3 tests/test_padfire_dialogs.py
python3 -m py_compile padfire.py tools/padfire-shot tests/test_padfire_core.py tests/test_padfire_dialogs.py packaging/extra-scripts/padfire_test.py
bash -n padfire-launch packaging/extra-scripts/padfire-restart.sh
shellcheck padfire-launch packaging/extra-scripts/padfire-restart.sh
desktop-file-validate packaging/padfire.desktop
git diff --check
```

The core tests use a temporary home directory and stub devices/processes.
The GTK dialog suite runs under a virtual display. Neither test suite is a
substitute for testing the physical Launchpad and real audio routes.

## Manual acceptance

Before tagging a release, verify on supported hardware:

- Pad presses, releases, top-row page changes and side-column mapping.
- LED states for assigned, empty, playing, stop and page indicators.
- USB unplug/replug while the application remains open; the top-bar badge
  should change to Disconnected and return to Connected without relaunch.
- Real sound playback, stop, loop/retrigger and monitor mirroring.
- Settings persistence, restart and a clean application quit.

Do not perform physical pad presses or audio playback in automated tests.

## Configuration and privacy

Pad configurations may contain local file paths and executable shell commands.
Never attach a real `padfire.json` to an issue or pull request without
reviewing and redacting it. Treat imported script-pad assignments as code.
