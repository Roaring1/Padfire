# Security

## Supported security model

Padfire is a local, single-user desktop application. It is not a network service
and is not designed to be exposed to other users or machines.

- Runtime state is stored under `~/.config/padfire`. Current versions create
  that directory with owner-only permissions and write the config, backup,
  PID marker and Unix socket with owner-only access. Existing legacy backups
  are tightened to owner-only mode on startup. Config files are limited to
  4 MiB and are read as regular files only; symlinks, hard links, FIFOs and
  other special files are rejected.
- The IPC socket accepts a small fixed command set from the same user. It is
  protected by filesystem permissions, bounded request/response sizes and a
  limit on concurrent clients. It does not provide authentication between
  processes running as the same user.
- Sound playback and loudness analysis pass file paths as argument-vector
  entries rather than interpolating them into shell commands.
- The per-pad **Script** action is intentionally different: it executes the
  command the user configured through the user's shell. A pad configuration
  can therefore run arbitrary commands with the privileges of the Padfire
  user. Only import or share configurations you trust.
- The `tools/padfire-shot` helper only writes PNG files directly inside
  the existing home, Pictures, Desktop or system temp directories. It builds
  paths from that fixed directory allow-list and rejects symlinks, hard links,
  special files and targets owned by another user.
- Do not publish `padfire.json`, screenshots containing private paths, or
  shell-command assignments without reviewing them first.

## Reporting a vulnerability

Please do not publish an exploitable issue or proof of concept in a public
issue. Use GitHub's **Report a vulnerability** flow for this repository. Private
vulnerability reporting is enabled. Do not open a public issue for an
unpatched security vulnerability.

There is no formal response-time or remediation SLA at this stage. Include the
affected version/commit, impact, reproduction steps and any relevant logs,
but remove personal paths, tokens and private configuration first.
