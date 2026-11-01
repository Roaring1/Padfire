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
    X display number or Wayland socket is assumed.

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

The live pad assignments/configuration are intentionally not included in the
repository. They live at ~/.config/padfire/padfire.json and can contain
machine-specific audio paths, sound-file paths and local commands.
