# Padfire Rust rewrite

This is the parallel replacement implementation for Padfire 3.x. The released v2.4.0 application remains the production reference and must remain installed until every gate in `CUTOVER.md` has passed.

The goal is one shared Rust application source tree for Windows 11 and Linux/Nobara. Domain rules, configuration, pad actions and MIDI protocol are shared. OS-specific storage, process lifecycle, audio devices and IPC stay behind platform adapters.

## Workspace

- `crates/padfire-core`: configuration schema/migration, action semantics, audio routing decisions, MIDI packet model and IPC contract.
- `crates/padfire-platform`: secure config storage, MIDI backend/reconnect worker, audio playback engine, process supervision and local IPC transports.
- `apps/padfire`: eframe/egui desktop shell, pad editor/settings, app runtime and CLI.

## Build and validate

From this directory:

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked
```

CI runs the same gates on Ubuntu and Windows. The current local environment is Nobara Linux; Windows native compilation and behavior must be proven by the Windows CI runner and the actual Windows laptop before cutover.

## Compatibility and safety invariants

- Preserve the released v5 JSON config and allow rollback to v2.4.0 without data loss.
- Preserve 8 pages, the 8x8 pad grid, right-side column, CC page selection and per-pad behavior.
- Keep Mini MK1/MK2 note and LED protocol. Reject MK3 before opening an output port; never probe by sending speculative LED messages.
- Make playback and script operations generation-safe. Stop must defeat work still decoding, opening a device or registering a worker.
- Keep user-authored shell scripts an explicit feature with a visible trust boundary.
- Keep IPC local-only, user/owner restricted, bounded and version-compatible.
- A new installation uses the current system default output; never bake machine-specific sink names into portable defaults.
- LUFS analysis is opt-in and uses FFmpeg when installed; it analyzes at most the first 120 seconds, targets -18 LUFS and never silently changes pad volume. Applying the suggested 1-150% gain is an explicit editor action.
- The playback meter taps Padfire's decoded/mixed signal after pad-volume scaling, aggregates in 256-sample chunks with one atomic update per chunk, uses fast-attack/slow-release smoothing, and returns to zero while idle. It does not launch a monitor process or measure unrelated system audio/post-device effects.
- Do not add automatic startup or an always-running helper.
- Never replace the production launcher or running process during development.

See `DECISIONS.md`, `BEHAVIOR_MATRIX.md`, `MAP.md`, `CUTOVER.md` and `research/STACK.md`.
