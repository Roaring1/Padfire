Padfire packaging
=================

Linux
-----
padfire.desktop
    Desktop launcher entry. Uses Exec=padfire-launch and the theme icon
    audio-x-generic.

padfire.service
    Optional systemd --user unit; depends on the logged-in graphical session
    and PipeWire / WirePlumber.

extra-scripts/padfire_test.py
    Standalone Launchpad Mini MIDI protocol smoke test. It sends LED messages
    to hardware and is intentionally not run in automated CI.

extra-scripts/padfire-restart.sh
    Linux developer helper for syntax-checking and restarting the GTK app.

Windows
-------
windows/requirements.txt
    Runtime and build dependencies for the native PySide6 / PortAudio front end.

windows/build.ps1
    PowerShell build/test entry point. Creates a one-folder GUI distribution at
    dist/Padfire/Padfire.exe and a separate console CLI at dist/PadfireCLI/.

windows/Padfire.spec
    PyInstaller one-folder GUI application definition.

windows/Padfire.iss
    Optional Inno Setup installer definition. It is per-user and does not
    require administrator privileges.

The live configuration file is never stored in the repository. It contains
user-specific audio paths and script commands. The portable data contract is
the version-5 padfire.json schema documented in the root README.

    windows/PadfireCLI.spec
        Separate console executable for --status / --show / --stop / --reload /
        --quit / startup management. The GUI exe stays windowed and has no
        attached console.
