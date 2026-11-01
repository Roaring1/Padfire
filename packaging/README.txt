Padfire packaging assets
========================

padfire.desktop
    Freedesktop launcher for KDE, GNOME and other XDG desktop environments.
    Uses the theme icon name "padfire", a matching StartupWMClass, an explicit
    TryExec and quick actions for Show, Stop All and Quit. The installer
    resolves the launcher path for the current user and writes the entry to
    ~/.local/share/applications.

icons/hicolor/scalable/apps/padfire.svg
    Bundled scalable app icon. The installer places it in the user's hicolor
    icon theme, so the launcher and window use the same identity without
    relying on a machine-specific absolute icon path.

padfire.service
    User systemd unit template. Uses %h/bin/padfire-launch and inherits the
    active desktop session's display/audio environment; no fixed username,
    X display number or Wayland socket is assumed. It orders startup after
    PipeWire, pipewire-pulse and WirePlumber.

extra-scripts/padfire-restart.sh
    Optional developer/maintenance helper. It runs a syntax check, asks an
    existing Padfire instance to quit through its own IPC command, waits for
    that instance to disappear, then launches the checkout. It never uses a
    broad pkill pattern and never unlinks a live socket. Logs are created with
    owner-only permissions. Review PADFIRE_PY, PADFIRE_LAUNCH and PADFIRE_LOG
    before using environment overrides.

extra-scripts/padfire_test.py
    Optional manual Launchpad Mini MK1/MK2 hardware smoke test. It deliberately
    lights the controller and opens both MIDI ports. Quit Padfire first; the
    script checks the local IPC socket and refuses to claim ports if Padfire
    is running or socket ownership cannot be established. The Mini MK3 is
    explicitly excluded because it uses a different protocol. This tool is
    not part of automated CI.

Install on Linux
----------------

Install the runtime requirements listed in README.md, then from the checkout:

    ./padfire-launch --install
    systemctl --user start padfire

The install command writes the per-user desktop entry and icon, creates or
updates the user service, imports the current desktop-session environment,
and enables the service for the graphical session. It does not create a new
XDG autostart entry or change audio routing preferences.

Run without installing:

    ./padfire-launch

Configuration and privacy
-------------------------

Pad assignments/configuration are intentionally not included in the
repository. They live at ~/.config/padfire/padfire.json and may contain
machine-specific audio paths, sound-file paths and executable shell commands.

Current Padfire versions keep ~/.config/padfire private (0700) and write the
configuration, backup, PID marker and IPC socket with owner-only permissions
(0600). Config reads are capped at 4 MiB and reject symlinks, hard links and
special files. A Script pad executes
the shell command saved by the user; treat imported configurations as
executable content and only use configurations from sources you trust.

See README.md, SECURITY.md, CONTRIBUTING.md and RELEASING.md for the full
runtime, security, testing and release guidance.
