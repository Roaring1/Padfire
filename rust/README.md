# Padfire Rust rewrite

This is the replacement implementation for Padfire 3.x. The released v2.4.0
application remains the behavioral reference and the production installation
until the Rust binary has passed the gates in CUTOVER.md.

The goal is one pure-Rust application source tree that builds for Windows 11 and
Linux/Nobara. The UI, configuration model, action semantics and device protocol
are shared. Operating-system differences belong behind narrow adapters.

## Build

From this directory:

\`\`\`sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
\`\`\`

The platform-independent core is being ported first. GUI and platform crates
are added in stages so the compatibility contract can be tested before it is
coupled to native windows, audio servers or physical devices.

## Non-negotiable compatibility

- Read and write the released v5 JSON configuration format during the port.
  Do not force a one-way migration while v2.4.0 remains installed.
- Keep the Mini MK1/MK2 note and LED protocol. Reject MK3 before opening an
  output port; never probe a device by sending speculative LED messages.
- Preserve the 8 pages, 8x8 grid, right-side column, CC page selection and
  per-pad behavior.
- Keep audio operations cancellable and generation-safe. Stop must defeat a
  playback that is still decoding, opening a device or registering a worker.
- User-authored script pads remain an explicit shell-execution feature, with
  the trust boundary visible in the UI and documentation.
- IPC stays local-only, owner/user restricted, bounded and version-compatible.
- A blank/new installation routes to the current system default; never bake
  this machine's sink names into a portable default config.
- No automatic startup or always-running helper is introduced by installation.

See DECISIONS.md, BEHAVIOR_MATRIX.md, MAP.md and research/STACK.md.
