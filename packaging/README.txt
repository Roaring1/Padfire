Packaging extras (not part of the git repo)
============================================

padfire.desktop     - Desktop launcher entry. Uses Exec=padfire-launch so it
                       works when padfire-launch is installed in PATH (for
                       example ~/bin). Uses the theme icon audio-x-generic so
                       it does not depend on a missing local icon file.
padfire.service     - systemd --user unit (autostart). ExecStart uses
                       %h/bin/padfire-launch, avoiding a hardcoded username.
                       It depends on PipeWire / pipewire-pulse / WirePlumber
                       and otherwise leaves display/session variables to the
                       user service environment.
extra-scripts/padfire_test.py
                    - Standalone MIDI smoke-test script (not wired into the
                      app) for confirming the Launchpad Mini talks the
                      expected note/CC layout before debugging padfire.py
                      itself. Needs `mido`.
extra-scripts/padfire-restart.sh
                    - Dev helper: syntax-checks padfire.py, kills any
                      running instance, and relaunches it with logs tailed.
                      It resolves the repo root from its own location and can
                      be overridden with PADFIRE_PY, PADFIRE_LAUNCH,
                      PADFIRE_LOG, and PADFIRE_SOCK.

Deliberately NOT included: ~/.config/padfire/padfire.json (the live pad
config). It's full of this-machine's personal file paths (sound files under
~/Downloads, scripts under a specific mounted drive) and isn't relevant to
debugging Padfire's own code. Config schema, for reference, is a "pages"
list of dicts keyed by Launchpad note (grid row*16+col, side pads row*16+8),
each either:
  {"action": "play", "file": ..., "sink": ..., "vol": 0-150, "loop": bool,
   "label": ..., "retrigger": "global"|"restart"|"toggle", "color": ...,
   "notes": ...}
  {"action": "script", "cmd": ..., "pid_file": ..., "label": ...,
   "color": ..., "notes": ...}
  {"action": "stop_sfx"} / {"action": "stop_all"} /
  {"action": "stop_key", "stop_key": "<page>:<note>"}
Top-level keys also include "version", "default_sink",
"stop_on_page_change", "retrigger", "monitor_enabled", "monitor_sink".

Install on the second PC:
  git clone git@github.com:Roaring1/Padfire.git
  # or: git clone https://github.com/Roaring1/Padfire.git
  cd Padfire
  ./padfire-launch            # sets JACK env suppression, then runs padfire.py
  # optionally: ln -s "$(pwd)/padfire-launch" ~/bin/padfire-launch
  # then install packaging/padfire.desktop into ~/.local/share/applications/,
  # and packaging/padfire.service into ~/.config/systemd/user/ if you want
  # autostart.
