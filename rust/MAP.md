# Padfire implementation map

| Area | Implementation | Automated verification | Remaining manual acceptance |
|---|---|---|---|
| Domain/config | padfire-core; v5 schema, bounded parsing, migration, typed actions | Config, compatibility and action tests | Review actual user config changes when schema changes |
| Platform | padfire-platform; secure filesystem, IPC, process supervision, MIDI and audio backends | Platform and integration tests on Linux/Windows | Real hardware and audio behavior |
| Desktop app | apps/padfire; native eframe/egui UI, tray, editor/settings and CLI | Xvfb rendering and CLI tests | Visible KDE Wayland and Windows UI/tray |
| MIDI | Launchpad Mini MK1/MK2 note/CC protocol and reconnect worker | Hardware-free parsing, selection and reconnect-state tests | Pad input, LEDs and physical reconnect |
| Audio | PipeWire/Pulse compatibility on Linux; WASAPI on Windows; per-pad routing, monitor and meters | Audio engine and routing tests | Real playback, loopback and VU |
| Installer | Per-user binary, desktop entry/icon and third-party license notice | Installer lifecycle, rollback-on-failure and report-byte tests | Stable release signing/provenance |
| Security | Bounded config/IPC, safe path/process handling, dependency policy | CodeQL all languages, cargo-deny, strict Clippy | Monitor findings as code evolves |

The native Rust implementation is the only active Padfire app. Legacy GTK/Python code, packaging, tests and workflow have been removed from main; historical records remain in Git and the dated changelog.
