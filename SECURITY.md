# Security

## Security model

Padfire is a local, single-user desktop application, not a network service. The current native Rust implementation stores its configuration under ~/.config/padfire-rust and uses a same-user local IPC transport. It does not open a network listener.

- Configuration is capped at 4 MiB, parsed with depth/node limits and normalized before use. Writes are atomic; the active v5 configuration and rolling backup are owner-restricted. Symlinks, hard links, special files and foreign-owned paths are rejected where applicable.
- Linux IPC uses a private Unix socket; Windows uses a current-user named pipe. Requests, responses, concurrent clients and deadlines are bounded.
- Sound file and analysis paths are passed as separate process arguments, not interpolated into shell commands. Crosshair is resolved to a canonical executable from PATH and launched with a fixed argument.
- The per-pad Script action is intentionally different: it executes the command explicitly configured by the user through the user's shell. Pad configurations are executable content. Only import/share configs from trusted sources and review script assignments before triggering them.
- Process management has timeouts, output bounds and managed-child cleanup. No runtime audio samples are written to disk by the output meter.
- Third-party Rust dependencies are checked by the pinned cargo-deny advisory/license/ban/source policy. The bundled license notice is generated from Cargo.lock and byte-verified in CI.

Do not publish padfire.json, screenshots containing private paths, tokens, or shell-command assignments without reviewing and redacting them.

## Reporting a vulnerability

Please do not publish an exploitable issue or proof of concept in a public issue. Use GitHub's Report a vulnerability flow for this repository. Private vulnerability reporting is enabled. Include the affected version/commit, impact, reproduction steps and relevant logs, but remove personal paths, tokens and private configuration first.
