# Rust migration map

Legend: not started, in progress, implemented, verified, blocked.

| Unit | State | Owner crate/module | Gate |
|---|---|---|---|
| Config v5 model and migration | implemented | padfire-core::config | Rust fixture tests pass; compare real legacy fixture data without exposing its contents |
| MIDI packet decoder and Mini profile safety | implemented | padfire-core::midi | deterministic protocol tests pass |
| App state and pad action dispatcher | not started | padfire-core::actions | behavior/race parity tests |
| Secure config store and backup | not started | padfire-platform::config_store | symlink/hardlink/ACL/atomic replacement tests |
| Local IPC and CLI compatibility | not started | padfire-platform::ipc, padfire | golden JSON plus Unix socket/named pipe integration |
| MIDI backend and reconnect supervisor | not started | padfire-platform::midi | mock state machine, both OS builds and physical MK1/MK2 |
| Audio decode/mixer/routing | not started | padfire-platform::audio | fake backend plus PipeWire/WASAPI integration |
| VU capture | not started | padfire-platform::meter | PCM/RMS and idle lifecycle tests |
| Process supervision/script actions | not started | padfire-platform::process | shell-free Crosshair plus script process-tree tests |
| Loudness analyzer | not started | padfire-platform::loudness | compare against FFmpeg reference fixtures |
| Native UI shell and visual design | not started | padfire-ui | headless layout tests plus visual acceptance |
| Pad editor/settings | not started | padfire-ui | preserve editor fields and compact layouts |
| Tray, window controls and keyboard | not started | padfire-ui::integration | KDE/Windows visible acceptance |
| Linux packaging | not started | packaging/linux | clean Nobara install/upgrade/rollback |
| Windows packaging | not started | packaging/windows | clean Windows 11 install/upgrade/uninstall |
| CI, security and release pipeline | not started | .github/workflows | Linux + Windows matrix, fmt/clippy/tests, CodeQL and audit |

## Dependency order

1. Shared domain and config compatibility.
2. Filesystem/security and process lifecycle.
3. IPC/CLI and MIDI lifecycle.
4. Audio backend and VU.
5. UI, tray and native desktop integration.
6. Packaging, CI, dual-machine acceptance and v3.0 release.

Parallel work is safe only after the core types/events have stabilized. Audio
and GUI must not invent separate action/config models.
