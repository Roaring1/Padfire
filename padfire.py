#!/usr/bin/env python3
# padfire.py -- Launchpad Mini soundboard
# launch via ~/bin/padfire-launch (sets JACK env before import)
#
# - MIDI: Launchpad Mini MK1/MK2, X-Y note=row*16+col
# - top row CC 104-111 = pages 1-8, side col = assignable pads
# - LED: velocity=(green&3)<<4|(red&3)|0x0C  (0x0C = double-buffer, no CC 0 reset)
# - audio: paplay subprocess -> PipeWire, no PortAudio/JACK

import os, sys, json, time, math, shutil, struct, select, signal, socket, threading, argparse, subprocess
sys.stdout.reconfigure(line_buffering=True)

import mido
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib, Pango

HAVE_IND = False
try:
    gi.require_version("AppIndicator3", "0.1")
    from gi.repository import AppIndicator3 as AI
    HAVE_IND = True
except Exception:
    AI = None

VER      = "2.3"
HOME     = Path.home()
CFGDIR   = HOME / ".config" / "padfire"
SOCK     = str(CFGDIR / "padfire.sock")
PIDF     = str(CFGDIR / "padfire.pid")
CONF     = str(CFGDIR / "padfire.json")
HEARTH_SOCK = str(HOME / ".config" / "hearth" / "rac.sock")
RAC_SOCK = HEARTH_SOCK  # alias -- where hearth's IPC socket lives

# One-time migration: move config from old ~/.config/roaring/ location
_OLD_CFGDIR = HOME / ".config" / "roaring"
for _old, _new in [
    (_OLD_CFGDIR / "padfire.json", CFGDIR / "padfire.json"),
]:
    if _old.exists() and not _new.exists():
        CFGDIR.mkdir(parents=True, exist_ok=True)
        shutil.copy2(str(_old), str(_new))
        print(f"[config] migrated {_old} -> {_new}")

# The old dir also held a socket + pid file nothing reads any more; leaving
# them behind made it look like another instance was live.
if (CFGDIR / "padfire.json").exists():
    for _stale in ("padfire.sock", "padfire.pid"):
        try:
            _sp = _OLD_CFGDIR / _stale
            if _sp.exists(): _sp.unlink()
        except Exception: pass

DEVICE_SUBSTR = "Launchpad Mini"
# The Mini MK3 also reports as "Launchpad Mini" but uses a different
# protocol (notes 11-88, programmer mode), so matching it would send it
# garbage notes and LED bytes. Skip it until real MK3 support exists.
DEVICE_EXCLUDE = ("MK3", "mk3", "Mk3")
NUM_PAGES = 8
NUM_ROWS  = 8
NUM_COLS  = 8
PAD_PX    = 44

# LED constants -- formula: (green&3)<<4 | (red&3) | 0x0C
# 0x0C bits MUST be set or the Mini flickers (double-buffer flags)
LED_OFF    = 0x0C
LED_DIM    = 0x1C  # dim green -- assigned idle
LED_GREEN  = 0x3C  # full green -- playing / script active
LED_RED    = 0x0F  # full red -- stop pad
LED_ORANGE = 0x1F  # orange -- script pad idle
LED_AMBER  = 0x3F  # amber -- current page
LED_PG_HAS = 0x0D  # dim red -- page has pads but not current

NOTE_XY   = lambda row, col: row * 16 + col
NOTE_SIDE = lambda row: row * 16 + 8
CC_TOP    = list(range(104, 112))

# Physical LED colours the user can pick -- name -> (led_byte, css_bg, css_border, css_fg)
# Every entry corresponds to a real LED state on the hardware
PAD_COLORS = {
    "default":  (None,    None,                       None,                        None),
    "green":    (LED_GREEN, "rgba(0,100,30,0.65)",    "rgba(48,209,88,0.50)",     "rgba(220,255,220,0.90)"),
    "orange":   (LED_ORANGE,"rgba(100,45,0,0.65)",   "rgba(255,140,30,0.50)",    "rgba(255,200,100,0.90)"),
    "red":      (LED_RED,   "rgba(90,8,8,0.65)",     "rgba(255,69,58,0.50)",     "rgba(255,160,150,0.90)"),
    "amber":    (LED_AMBER, "rgba(90,65,0,0.65)",    "rgba(255,180,30,0.50)",    "rgba(255,230,120,0.90)"),
    "dim green":(LED_DIM,   "rgba(0,55,18,0.55)",    "rgba(48,209,88,0.25)",     "rgba(160,240,160,0.75)"),
}

TARGET_LUFS = -18.0

# CSS -- one accent color (#0a84ff blue). Pads differentiated by border only, not background fill.
CSS = """
* { font-family: "Cantarell","Noto Sans",sans-serif; }
window { background: #1a1a1c; }
/* `togglebutton` is not a valid GTK 3.20+ node, so this rule was dropped
   entirely. ToggleButton renders as a `button` node. */
button { transition: background 120ms ease, border-color 120ms ease, box-shadow 120ms ease; }

.topbar {
    background: #232325;
    border-bottom: 1px solid rgba(255,255,255,0.06);
    padding: 2px 6px;
    min-height: 28px;
}
.app-logo { font-size: 10px; font-weight: 800; letter-spacing: 4px; color: #0a84ff; }
.midi-ok  { color: #30d158; font-size: 8px; font-weight: 700; padding: 2px 7px;
            border-radius: 8px; background: rgba(48,209,88,0.12); }
.midi-bad { color: rgba(255,255,255,0.30); font-size: 8px; padding: 2px 7px;
            border-radius: 8px; background: rgba(255,255,255,0.05); }

.grid-wrap { padding: 6px; }

.pad-btn {
    border-radius: 6px;
    border: 1px solid rgba(255,255,255,0.07);
    background: rgba(34,34,38,0.95);
    min-width: 44px; min-height: 44px;
    font-size: 8px; font-weight: 600; letter-spacing: 0px;
    color: rgba(255,255,255,0.22);
    padding: 2px; margin: 0;
}
.pad-btn:hover  { background: rgba(50,50,56,0.95); border-color: rgba(255,255,255,0.13); }
.pad-btn:active { background: rgba(70,70,80,1.0); }

.pad-has     { border-color: rgba(48,209,88,0.30); color: rgba(255,255,255,0.78); }
/* State rings are an inset box-shadow, not border-color: the per-pad colour
   provider sets border-color at USER priority and used to completely hide
   "playing" on any pad that had a colour assigned. */
.pad-playing { box-shadow: inset 0 0 0 2px #30d158; color: #ffffff; }
/* Dashed edge = broken/missing, which does not collide with the solid
   red of .pad-stop. Paired with the leading warning glyph in the label. */
.pad-missing { border-style: dashed; border-width: 2px;
               border-color: rgba(255,69,58,0.90);
               color: rgba(255,170,160,0.95); }
.pad-stop    { border-color: rgba(255,69,58,0.40); color: rgba(255,130,120,0.85); }
.pad-script  { border-color: rgba(255,160,30,0.40); color: rgba(255,200,80,0.80); }
.pad-script-on { box-shadow: inset 0 0 0 2px rgba(255,200,50,0.85); color: rgba(255,230,100,0.95); }
.pad-action  { border-color: rgba(10,132,255,0.45); color: rgba(120,190,255,0.90); }
.pad-flash   { background: rgba(255,255,255,0.18); border-color: rgba(255,255,255,0.55); }
.pad-dragover { border-color: rgba(10,132,255,0.75); border-width: 2px; }

/* Side column is narrower than the grid, but it is still a pad:
   match the grid's 6px radius instead of rounding into a lozenge. */
.side-pad { min-width: 34px; min-height: 44px; border-radius: 6px; }

.page-btn {
    border-radius: 12px;
    border: 1px solid rgba(255,255,255,0.07);
    background: rgba(30,30,34,0.80);
    min-width: 44px; min-height: 20px;
    font-size: 7px; font-weight: 700; letter-spacing: 0.5px;
    color: rgba(255,255,255,0.28); padding: 1px; margin: 0;
}
.page-btn:hover   { background: rgba(46,46,52,0.90); border-color: rgba(255,255,255,0.14); }
.page-active      { background: rgba(10,70,180,0.32); border-color: rgba(10,132,255,0.55); color: #4db0ff; }
.page-has         { border-color: rgba(10,132,255,0.22); color: rgba(77,176,255,0.50); }

.statusbar {
    background: #171719;
    border-top: 1px solid rgba(255,255,255,0.05);
    padding: 2px 8px; min-height: 20px;
}
.stat-ok  { color: #30d158; font-size: 8px; }
.stat-dim { color: rgba(255,255,255,0.24); font-size: 8px; }
.stat-val { color: rgba(255,255,255,0.55); font-size: 8px; }
.stat-warn { color: #ff9f0a; font-size: 8px; font-weight: 700; }
.stat-play { color: #30d158; font-size: 8px; font-weight: 700; }

.lbl { color: rgba(255,255,255,0.32); font-size: 9px; }

button.act {
    background: rgba(255,255,255,0.06);
    border: 1px solid rgba(255,255,255,0.09);
    border-radius: 6px; padding: 3px 9px;
    color: rgba(255,255,255,0.65); font-size: 9px;
}
button.act:hover { background: rgba(255,255,255,0.11); }
/* A real "engaged toggle" style. v2.2 put .stat-ok on the monitor
   ToggleButton, but .stat-ok only sets colour + font-size, so the button kept
   the theme's default toggle background -- bright pink on Adwaita dark. */
button.act-on {
    background: rgba(48,209,88,0.18);
    border: 1px solid rgba(48,209,88,0.55);
    border-radius: 6px; padding: 3px 9px;
    color: #30d158; font-size: 9px; font-weight: 700;
}
button.act-on:hover { background: rgba(48,209,88,0.28); }
.btn-stop {
    background: rgba(255,69,58,0.15); border: 1px solid rgba(255,69,58,0.30);
    border-radius: 6px; padding: 3px 9px;
    color: #ff453a; font-size: 9px; font-weight: 700;
}
.btn-stop:hover { background: rgba(255,69,58,0.26); }
.btn-ok {
    background: #0a84ff; color: white; border: none;
    font-weight: 700; font-size: 9px; padding: 5px 14px; border-radius: 7px;
}
.btn-ok:hover { background: #2a9aff; }
.btn-sm { padding: 2px 7px; font-size: 8px; min-width: 0; border-radius: 5px; }

#vu trough { background: rgba(255,255,255,0.07); border-radius: 2px; min-height: 4px; }
#vu progress { background: linear-gradient(to right,#30d158,#ff9f0a 65%,#ff453a); border-radius: 2px; }
"""

CONFIG_VERSION = 5

DEFAULT_CONFIG = {
    "version":             CONFIG_VERSION,
    "default_sink":        "vm_game",
    "stop_on_page_change": False,
    "retrigger":           "restart",
    "monitor_enabled":     False,
    "monitor_sink":        "alsa_output.usb-Astro_Gaming_Astro_A50-00.stereo-game",
    "pages":               [{} for _ in range(NUM_PAGES)],
}

def _migrate(c):
    v = c.get("version", 1)
    while len(c.get("pages", [])) < NUM_PAGES:
        c.setdefault("pages", []).append({})
    if v < 4:
        c.setdefault("monitor_enabled", False)
        c.setdefault("monitor_sink", "")
        for page in c.get("pages", []):
            for pad in page.values():
                pad.setdefault("action", "play")
    # v2.2 rewrote a global "toggle" back to "restart" on every upgrade from
    # v<5, silently discarding a setting the dialog itself offers.
    if c.get("retrigger") not in ("restart", "toggle"):
        c["retrigger"] = "restart"
    # Drop keys old builds wrote but nothing ever read, so the config stops
    # carrying dead entries that look meaningful when you open the file.
    c.pop("device", None)
    for page in c.get("pages", []):
        for pad in page.values():
            if isinstance(pad, dict): pad.pop("mode", None)
    c["version"] = CONFIG_VERSION
    return c

def load_config():
    try:
        c = json.loads(Path(CONF).read_text())
        return _migrate(c)
    except Exception:
        return json.loads(json.dumps(DEFAULT_CONFIG))

def save_config(cfg):
    # atomic write + single rolling backup so a crash mid-save can't corrupt
    # the file that holds every pad assignment
    CFGDIR.mkdir(parents=True, exist_ok=True)
    dst = Path(CONF)
    tmp = Path(str(dst) + ".tmp")
    # flush + fsync before the rename: without it, power loss between write()
    # and rename() can leave a 0-byte config, losing every pad assignment
    with open(tmp, "w") as fh:
        fh.write(json.dumps(cfg, indent=2))
        fh.flush(); os.fsync(fh.fileno())
    # copy (not move) the old file to .bak so CONF never stops existing --
    # between the two os.replace calls there was a window with no config
    if dst.exists():
        try: shutil.copy2(str(dst), str(dst) + ".bak")
        except Exception as e: print(f"[config] backup failed: {e}")
    os.replace(str(tmp), str(dst))
    try:
        _dfd = os.open(str(CFGDIR), os.O_RDONLY); os.fsync(_dfd); os.close(_dfd)
    except Exception: pass


def crosshair_toggle_argv():
    """Return a safe argv for the installed Crosshair CLI, or None if absent."""
    executable = shutil.which("crosshair")
    if executable:
        return [executable, "--toggle"]
    fallback = Path.home() / ".local" / "bin" / "crosshair"
    if fallback.is_file() and os.access(fallback, os.X_OK):
        return [str(fallback), "--toggle"]
    return None


def _signal_process(proc, sig, *, process_group=False):
    """Signal a child, or its dedicated process group when it owns one."""
    if process_group and os.name == "posix":
        pid = getattr(proc, "pid", None)
        if pid:
            try:
                os.killpg(pid, sig)
                return
            except ProcessLookupError:
                return
            except OSError:
                pass
    try:
        if sig == signal.SIGTERM:
            proc.terminate()
        else:
            proc.kill()
    except Exception:
        pass


def _terminate_process(proc, timeout=0.5, *, process_group=False):
    """Terminate and reap a tracked child without leaving a zombie behind."""
    try:
        if proc.poll() is None:
            _signal_process(proc, signal.SIGTERM, process_group=process_group)
    except Exception:
        pass
    try:
        proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        _signal_process(proc, signal.SIGKILL, process_group=process_group)
        try:
            proc.wait(timeout=timeout)
        except Exception:
            pass
    except Exception:
        pass
    else:
        # The shell may exit on SIGTERM while a child ignores it. The process
        # group is still ours, so clear any remaining descendants after grace.
        if process_group:
            _signal_process(proc, signal.SIGKILL, process_group=True)

def _run_script_test_command(command, cancel_event, timeout=30.0):
    """Run the editor's Test Run command with bounded, cancellable cleanup."""
    if cancel_event.is_set():
        return
    proc = None
    process_group = os.name == "posix"
    try:
        proc = subprocess.Popen(
            command, shell=True, start_new_session=process_group
        )
        if cancel_event.is_set():
            _terminate_process(proc, process_group=process_group)
            return
        deadline = time.monotonic() + timeout
        while not cancel_event.is_set():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(command, timeout)
            try:
                proc.wait(timeout=min(0.1, remaining))
                return
            except subprocess.TimeoutExpired:
                continue
        _terminate_process(proc, process_group=process_group)
    except subprocess.TimeoutExpired:
        print(f"[script-test] timed out: {command[:100]}")
        if proc is not None:
            _terminate_process(proc, process_group=process_group)
    except Exception as exc:
        print(f"[script-test] failed: {exc}")




# audio engine -- paplay subprocess, no PortAudio

class AudioEngine:
    def __init__(self):
        self._procs = {}
        self._generations = {}
        self._lock  = threading.Lock()
        self._sinks = []
        self._refresh_sinks()

    def _refresh_sinks(self):
        try:
            out = subprocess.check_output(["pactl","list","short","sinks"],
                                          stderr=subprocess.DEVNULL, timeout=4).decode()
            self._sinks = [ln.split()[1] for ln in out.splitlines() if len(ln.split()) >= 2]
            print(f"[audio] {len(self._sinks)} sinks")
        except Exception as e:
            print(f"[audio] pactl failed: {e}")

    def _find_sink(self, name):
        lo = str(name or "").lower()
        for s in self._sinks:
            if s == name: return s
        for s in self._sinks:
            if s.lower().startswith(lo): return s
        for s in self._sinks:
            if lo and lo in s.lower(): return s
        return self._sinks[0] if self._sinks else "vm_game"

    @staticmethod
    def _stop_process(proc):
        try:
            if proc.poll() is None:
                proc.kill()
        except Exception:
            pass
        try:
            proc.wait(timeout=0.5)
        except Exception:
            pass

    def _cancel(self, keys):
        """Invalidate pending starts as well as terminate registered processes."""
        processes = []
        tokens = {}
        with self._lock:
            for key in keys:
                token = self._generations.get(key, 0) + 1
                self._generations[key] = token
                tokens[key] = token
                proc = self._procs.pop(key, None)
                if proc is not None:
                    processes.append(proc)
        for proc in processes:
            self._stop_process(proc)
        return tokens

    def play(self, key, filepath, sink_name, vol, loop, monitor_sink=None):
        monitor_key = f"__mon__{key}"
        tokens = self._cancel((key, monitor_key))
        resolved = self._find_sink(sink_name)
        # the dialog allows 0-150%, so cap there. The old 131072 (200%)
        # ceiling was unreachable from the UI.
        pa_vol   = int(min(65536 * vol / 100.0, int(65536 * 1.5)))
        print(f"[audio] {key} -> {resolved} vol={vol}%")

        def _run(tkey, sink, token):
            while True:
                # A Stop or newer Play may arrive before this worker starts.
                with self._lock:
                    if self._generations.get(tkey) != token:
                        return
                proc = None
                try:
                    proc = subprocess.Popen(
                        ["paplay", f"--device={sink}", f"--volume={pa_vol}", filepath],
                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                    with self._lock:
                        current = self._generations.get(tkey) == token
                        if current:
                            self._procs[tkey] = proc
                    if not current:
                        # Stop raced with Popen: never publish or leave this
                        # late process running after the caller asked to stop.
                        self._stop_process(proc)
                        return
                    _, err = proc.communicate()
                    rc = proc.returncode
                    if err and rc != 0:
                        print(f"[audio] paplay ({tkey}): {err.decode().strip()}")
                    with self._lock:
                        current = self._generations.get(tkey) == token
                        if self._procs.get(tkey) is proc:
                            self._procs.pop(tkey, None)
                    if not loop or not current:
                        break
                except Exception as e:
                    with self._lock:
                        current = self._generations.get(tkey) == token
                        if self._procs.get(tkey) is proc:
                            self._procs.pop(tkey, None)
                    if current:
                        print(f"[audio] {tkey}: {e}")
                    if proc is not None:
                        self._stop_process(proc)
                    break

        threading.Thread(target=_run, args=(key, resolved, tokens[key]),
                         daemon=True, name=f"audio-{key}").start()
        if monitor_sink and monitor_sink != sink_name:
            threading.Thread(
                target=_run,
                args=(monitor_key, self._find_sink(monitor_sink), tokens[monitor_key]),
                daemon=True, name=f"audio-monitor-{key}"
            ).start()

    def stop(self, key):
        self._cancel((key, f"__mon__{key}"))

    def stop_all(self):
        # Invalidate every known key under the same lock used to publish a
        # process. This also catches workers that have not reached Popen yet.
        with self._lock:
            keys = set(self._generations) | set(self._procs)
            processes = []
            for key in keys:
                self._generations[key] = self._generations.get(key, 0) + 1
                proc = self._procs.pop(key, None)
                if proc is not None:
                    processes.append(proc)
        for proc in processes:
            self._stop_process(proc)

    def is_playing(self, key):
        with self._lock: proc = self._procs.get(key)
        return proc is not None and proc.poll() is None

    def list_sinks(self, refresh=False):
        if refresh or not self._sinks: self._refresh_sinks()
        return list(self._sinks)


# MIDI engine

class MidiEngine:
    def __init__(self, on_pad, on_top, on_side, on_connect, on_disconnect):
        self._on_pad = on_pad; self._on_top = on_top; self._on_side = on_side
        self._on_connect = on_connect; self._on_disconnect = on_disconnect
        self._in = self._out = None
        self._out_lock  = threading.Lock()
        self._conn_lock = threading.Lock()
        self._connected = False; self._running = True
        threading.Thread(target=self._read_loop,      daemon=True, name="midi-read").start()
        threading.Thread(target=self._reconnect_loop, daemon=True, name="midi-watch").start()

    def _find(self, names):
        for n in names:
            if DEVICE_SUBSTR not in n: continue
            if any(x in n for x in DEVICE_EXCLUDE):
                print(f"[midi] ignoring {n!r}: Mini MK3 uses a different protocol")
                continue
            return n

    def try_connect(self):
        with self._conn_lock:
            if not self._running: return False
            if self._connected: return True
            try:
                inn  = self._find(mido.get_input_names())
                outn = self._find(mido.get_output_names())
            except Exception as e:
                # Backend enumeration can fail transiently while ALSA/USB is
                # reinitializing. Keep the reconnect watcher alive to retry.
                print(f"[midi] discovery failed: {e}")
                return False
            if not inn or not outn: return False
            out_port = in_port = None
            try:
                out_port = mido.open_output(outn)
                in_port = mido.open_input(inn)
                self._out = out_port
                self._in = in_port
                self._connected = True
                print(f"[midi] connected: {inn}")
                self._on_connect()
                return True
            except Exception as e:
                # Opening the input can fail after the output has acquired the
                # device. Do not retain that half-open handle: reconnect runs
                # repeatedly and would otherwise leak/lock MIDI ports.
                for port in (in_port, out_port):
                    try:
                        if port: port.close()
                    except Exception:
                        pass
                self._in = self._out = None
                self._connected = False
                print(f"[midi] connect failed: {e}")
                return False

    def _disconnect(self):
        with self._conn_lock:
            was_connected = self._connected
            self._connected = False
            in_port, out_port = self._in, self._out
            self._in = self._out = None
            # Serialize output-port close with LED sends; cleanup also runs
            # after a partial connection failure, even when _connected=False.
            with self._out_lock:
                for port in (in_port, out_port):
                    try:
                        if port: port.close()
                    except Exception:
                        pass
        if was_connected:
            self._on_disconnect()

    def _reconnect_loop(self):
        while self._running:
            time.sleep(2)
            if not self._running:
                break
            if not self._connected:
                self.try_connect()

    def _read_loop(self):
        while self._running:
            if not self._connected or not self._in:
                time.sleep(0.05); continue
            try:
                for msg in self._in:
                    if not self._running: break
                    self._dispatch(msg)
            except Exception:
                if self._connected: self._disconnect()

    def _dispatch(self, msg):
        if msg.type == 'note_on':
            row, col = msg.note // 16, msg.note % 16
            pressed  = msg.velocity > 0
            if col == 8: self._on_side(row, pressed)
            else:        self._on_pad(row, col, pressed)
        elif msg.type == 'note_off':
            row, col = msg.note // 16, msg.note % 16
            if col == 8: self._on_side(row, False)
            else:        self._on_pad(row, col, False)
        elif msg.type == 'control_change' and msg.control in CC_TOP:
            self._on_top(msg.control - 104, msg.value > 0)

    def _send(self, msg):
        with self._out_lock:
            if self._out:
                try: self._out.send(msg)
                except Exception: pass

    def led(self, note, color):
        self._send(mido.Message('note_on', note=note, velocity=color, channel=0))

    def cc_led(self, cc, color):
        self._send(mido.Message('control_change', control=cc, value=color, channel=0))

    def clear_all(self):
        for row in range(NUM_ROWS):
            for col in range(NUM_COLS): self.led(NOTE_XY(row, col), LED_OFF)
            self.led(NOTE_SIDE(row), LED_OFF)
        for cc in CC_TOP: self.cc_led(cc, LED_OFF)

    def is_connected(self): return self._connected

    def shutdown(self):
        self._running = False
        if self._connected:
            try: self.clear_all()
            except Exception: pass
        self._disconnect()


# IPC

def ipc_send(cmd, sock_path=None, timeout=1.0):
    path = sock_path or SOCK
    if not Path(path).exists(): return None
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(timeout); s.connect(path)
        s.sendall((cmd + "\n").encode())
        # one recv() can return a partial line once the status payload grows,
        # which truncated the JSON that Hearth parses
        buf = b""
        while b"\n" not in buf:
            chunk = s.recv(4096)
            if not chunk: break
            buf += chunk
        s.close()
        return buf.decode(errors="replace").strip()
    except Exception: return None

def rac_send(cmd): return ipc_send(cmd, RAC_SOCK)

def ipc_serve(handler, srv_ref):
    sp = Path(SOCK); sp.parent.mkdir(parents=True, exist_ok=True)
    if sp.exists():
        try: sp.unlink()
        except Exception: pass
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try: srv.bind(str(sp)); srv.listen(4); os.chmod(str(sp), 0o600)
    except Exception: return
    srv_ref.append(srv); srv.settimeout(1.0)

    def _handle(conn):
        try:
            conn.settimeout(2.0); buf = b""
            while b"\n" not in buf:
                chunk = conn.recv(256)
                if not chunk: break
                buf += chunk
            resp = (handler(buf.decode(errors="replace").strip()) or "ok") + "\n"
            conn.sendall(resp.encode())
        except Exception: pass
        finally:
            try: conn.close()
            except Exception: pass

    while True:
        try:
            c, _ = srv.accept()
            threading.Thread(target=_handle, args=(c,), daemon=True).start()
        except socket.timeout: continue
        except Exception: break


# LUFS analysis via ffmpeg

def analyze_lufs(filepath):
    try:
        # -t 120 caps analysis: a long file blew the 30s timeout and was
        # reported to the user as "Could not read file"
        result = subprocess.run(
            ["ffmpeg", "-hide_banner", "-t", "120", "-i", filepath,
             "-filter:a", "loudnorm=print_format=json", "-f", "null", "-"],
            capture_output=True, text=True, timeout=30)
        raw = result.stderr
        start = raw.rfind("{"); end = raw.rfind("}") + 1
        if start == -1 or end == 0: return None
        data     = json.loads(raw[start:end])
        measured = float(data["input_i"])
        tp       = float(data["input_tp"])
        # a silent file reports "-inf", which raised OverflowError below and
        # surfaced as the same misleading "could not read" message
        if not (math.isfinite(measured) and math.isfinite(tp)):
            return {"silent": True}
        gain_lin = 10 ** ((TARGET_LUFS - measured) / 20.0)
        suggested = max(1, min(150, int(round(100.0 * gain_lin))))
        return {"input_i": measured, "input_tp": tp, "suggested_vol": suggested}
    except subprocess.TimeoutExpired:
        print("[lufs] ffmpeg timed out"); return None
    except Exception as e:
        print(f"[lufs] {e}"); return None


# pad edit dialog
# principle: show only what the user needs. advanced stuff is collapsed.

def pad_edit_dialog(parent, pad_cfg, sinks, default_sink, audio, all_pad_keys):
    dlg = Gtk.Dialog(title="Edit Pad", transient_for=parent, modal=True)
    dlg.set_default_size(400, -1)
    dlg.add_buttons("Cancel", Gtk.ResponseType.CANCEL, "Save", Gtk.ResponseType.OK)
    dlg.get_widget_for_response(Gtk.ResponseType.OK).get_style_context().add_class("btn-ok")
    dlg.set_default_response(Gtk.ResponseType.OK)

    box = dlg.get_content_area()
    box.set_spacing(8); box.set_margin_start(14); box.set_margin_end(14)
    box.set_margin_top(10); box.set_margin_bottom(10)

    def row(label, widget, parent_box=None):
        hb = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        lb = Gtk.Label(label=label); lb.set_xalign(0); lb.set_width_chars(9)
        lb.get_style_context().add_class("lbl")
        hb.pack_start(lb, False, False, 0); hb.pack_start(widget, True, True, 0)
        (parent_box or box).pack_start(hb, False, False, 0)

    action = (pad_cfg.get("action", "play") if pad_cfg else "play")

    # what this pad does
    act_c = Gtk.ComboBoxText()
    for txt in ["Play a sound", "Run a script", "Stop SFX", "Stop everything",
                "Stop a specific pad", "Toggle Crosshair"]:
        act_c.append_text(txt)
    act_map = {"play":0, "script":1, "stop_sfx":2, "stop_all":3,
               "stop_key":4, "crosshair_toggle":5}
    act_c.set_active(act_map.get(action, 0))
    row("Does", act_c)

    # label
    lbl_e = Gtk.Entry()
    lbl_e.set_text(pad_cfg.get("label","") if pad_cfg else "")
    lbl_e.set_placeholder_text("Name shown on the pad")
    lbl_e.set_activates_default(True)
    row("Name", lbl_e)

    # color -- named options only, matches physical LED
    color_names = list(PAD_COLORS.keys())
    col_c = Gtk.ComboBoxText()
    swatch = Gtk.Label(label=" ● ")
    for cn in color_names: col_c.append_text(cn)
    stored = (pad_cfg.get("color") if pad_cfg else None) or "default"
    if stored not in PAD_COLORS: stored = "default"
    col_c.set_active(color_names.index(stored))

    def _swatch(*_):
        idx = col_c.get_active()
        cn  = color_names[idx] if idx >= 0 else "default"
        _, bg, border, fg = PAD_COLORS.get(cn, PAD_COLORS["default"])
        if fg:
            solid = fg.replace("rgba","rgb").rsplit(",",1)[0] + ")"
            try: swatch.set_markup(f'<span foreground="{solid}"> ● </span>')
            except Exception: swatch.set_text(" ● ")
        else: swatch.set_text(" ● ")
    col_c.connect("changed", _swatch); _swatch()
    cbox = Gtk.Box(spacing=4)
    cbox.pack_start(col_c, True, True, 0); cbox.pack_start(swatch, False, False, 0)
    row("Color", cbox)

    # notes
    notes_buf = Gtk.TextBuffer()
    notes_buf.set_text(pad_cfg.get("notes","") if pad_cfg else "")
    notes_view = Gtk.TextView(buffer=notes_buf)
    notes_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR); notes_view.set_size_request(-1, 44)
    notes_view.get_style_context().add_class("lbl")
    ns = Gtk.ScrolledWindow(); ns.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
    ns.set_size_request(-1, 44); ns.add(notes_view)
    row("Notes", ns)

    sep1 = Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL)
    sep1.set_margin_top(2); sep1.set_margin_bottom(2)
    box.pack_start(sep1, False, False, 0)

    # play section
    play_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    box.pack_start(play_box, False, False, 0)
    def prow(l, w): row(l, w, play_box)

    file_btn = Gtk.FileChooserButton(title="Pick audio file", action=Gtk.FileChooserAction.OPEN)
    ff = Gtk.FileFilter(); ff.set_name("Audio")
    for p in ["*.wav","*.flac","*.ogg","*.mp3","*.aiff","*.opus"]: ff.add_pattern(p)
    file_btn.add_filter(ff)
    if pad_cfg and pad_cfg.get("file") and Path(pad_cfg["file"]).exists():
        file_btn.set_filename(pad_cfg["file"])
    prow("File", file_btn)

    # sink picker -- group virtual sinks first
    _priority = ("vm_", "astro", "scarlett", "b1", "b2", "laptop", "loopback")
    def _sort_sinks(sl):
        virt = [s for s in sl if any(k in s.lower() for k in _priority)]
        hw   = [s for s in sl if s not in virt]
        return virt, hw
    virt, hw = _sort_sinks(sinks)
    sink_c = Gtk.ComboBoxText()
    cur_sink = (pad_cfg.get("sink", default_sink) if pad_cfg else default_sink)
    combo_sinks = []
    for s in virt:
        sink_c.append_text(s); combo_sinks.append(s)
    if hw:
        sink_c.append_text("-- hardware --"); combo_sinks.append(None)
        for s in hw:
            sink_c.append_text(s); combo_sinks.append(s)
    try: sink_c.set_active(combo_sinks.index(cur_sink))
    except ValueError: sink_c.set_active(next((i for i, v in enumerate(combo_sinks) if v), 0))
    prow("Output", sink_c)

    def get_sink():
        idx = sink_c.get_active()
        if 0 <= idx < len(combo_sinks) and combo_sinks[idx]: return combo_sinks[idx]
        return default_sink

    vol_adj = Gtk.Adjustment(value=pad_cfg.get("vol",80) if pad_cfg else 80,
                             lower=0, upper=150, step_increment=1)
    vol_sl  = Gtk.Scale(orientation=Gtk.Orientation.HORIZONTAL, adjustment=vol_adj)
    vol_sl.set_value_pos(Gtk.PositionType.RIGHT); vol_sl.set_digits(0)
    prow("Volume", vol_sl)

    # LUFS / loudness suggestion
    lufs_row = Gtk.Box(spacing=6)
    la_btn = Gtk.Button(label="Check loudness"); la_btn.get_style_context().add_class("act")
    la_lbl = Gtk.Label(label=""); la_lbl.get_style_context().add_class("lbl"); la_lbl.set_xalign(0)
    ap_btn = Gtk.Button(label="Use"); ap_btn.get_style_context().add_class("act btn-sm")
    # set_visible(False) is undone by dlg.show_all(); without no_show_all the
    # "Use" button showed before any analysis and clicking it did nothing
    ap_btn.set_no_show_all(True); ap_btn.set_visible(False)
    _lufs  = [None]

    def _analyze(*_):
        f = file_btn.get_filename()
        if not f: return
        la_btn.set_sensitive(False); la_btn.set_label("Analyzing...")
        def _bg():
            r = analyze_lufs(f); GLib.idle_add(_done, r)
        def _done(r):
            la_btn.set_sensitive(True); la_btn.set_label("Check loudness")
            if not r: la_lbl.set_text("Could not read file"); return False
            if r.get("silent"):
                la_lbl.set_text("File is silent -- nothing to measure"); return False
            _lufs[0] = r
            la_lbl.set_markup(f"<b>{r['input_i']:.1f} LUFS</b>  peak {r['input_tp']:.1f} dBFS  -> suggested <b>{r['suggested_vol']}%</b>")
            ap_btn.set_visible(True); return False
        threading.Thread(target=_bg, daemon=True).start()

    la_btn.connect("clicked", _analyze)
    ap_btn.connect("clicked", lambda *_: _lufs[0] and vol_adj.set_value(_lufs[0]["suggested_vol"]))
    lufs_row.pack_start(la_btn, False, False, 0)
    lufs_row.pack_start(la_lbl, True, True, 0)
    lufs_row.pack_start(ap_btn, False, False, 0)
    play_box.pack_start(lufs_row, False, False, 0)

    # advanced options (collapsed by default)
    adv_exp = Gtk.Expander(label="Advanced")
    adv_exp.get_style_context().add_class("lbl")
    adv_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    adv_box.set_margin_top(4)
    adv_exp.add(adv_box)
    play_box.pack_start(adv_exp, False, False, 0)
    def arow(l, w): row(l, w, adv_box)

    loop_sw = Gtk.Switch(); loop_sw.set_active(pad_cfg.get("loop",False) if pad_cfg else False)
    lb2 = Gtk.Box(); lb2.pack_start(loop_sw, False, False, 0)
    arow("Loop", lb2)

    rt_c = Gtk.ComboBoxText()
    rt_c.append_text("Restart from start"); rt_c.append_text("Stop on 2nd press"); rt_c.append_text("Use global setting")
    rt_c.set_active({"restart":0,"toggle":1,"global":2}.get(pad_cfg.get("retrigger","global") if pad_cfg else "global", 2))
    arow("2nd press", rt_c)

    # test buttons
    tr = Gtk.Box(spacing=6)
    TK = "__test__"
    tb = Gtk.Button(label="Play"); tb.get_style_context().add_class("act")
    sb2 = Gtk.Button(label="Stop"); sb2.get_style_context().add_class("act")
    def _test(*_):
        f = file_btn.get_filename()
        if f: threading.Thread(target=lambda: audio.play(TK, f, get_sink(), int(vol_adj.get_value()), loop_sw.get_active()), daemon=True).start()
    tb.connect("clicked", _test)
    sb2.connect("clicked", lambda *_: threading.Thread(target=lambda: audio.stop(TK), daemon=True).start())
    tr.pack_start(tb, False, False, 0); tr.pack_start(sb2, False, False, 0)
    play_box.pack_start(tr, False, False, 0)

    # script section
    script_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    box.pack_start(script_box, False, False, 0)
    def srow(l, w): row(l, w, script_box)

    cmd_e = Gtk.Entry()
    cmd_e.set_text(pad_cfg.get("cmd","") if pad_cfg else "")
    cmd_e.set_placeholder_text("/path/to/script.sh")
    cmd_e.set_activates_default(True)
    srow("Command", cmd_e)

    br = Gtk.Box(spacing=6)
    bb = Gtk.Button(label="Browse..."); bb.get_style_context().add_class("act btn-sm")
    ts = Gtk.Button(label="Test run"); ts.get_style_context().add_class("act btn-sm")
    def _browse(*_):
        d2 = Gtk.FileChooserDialog(title="Choose script", transient_for=dlg, action=Gtk.FileChooserAction.OPEN)
        d2.add_buttons("Cancel", Gtk.ResponseType.CANCEL, "Select", Gtk.ResponseType.OK)
        if d2.run() == Gtk.ResponseType.OK: cmd_e.set_text(d2.get_filename())
        d2.destroy()
    bb.connect("clicked", _browse)
    _test_cancel = threading.Event()
    _test_thread: list[threading.Thread | None] = [None]

    def _test_run(*_):
        command = cmd_e.get_text().strip()
        if not command or _test_cancel.is_set():
            return
        current = _test_thread[0]
        if current is not None and current.is_alive():
            return
        worker = threading.Thread(
            target=_run_script_test_command,
            args=(command, _test_cancel),
            daemon=True,
            name="padfire-script-test",
        )
        _test_thread[0] = worker
        worker.start()

    ts.connect("clicked", _test_run)
    br.pack_start(bb, False, False, 0); br.pack_start(ts, False, False, 0)
    script_box.pack_start(br, False, False, 0)

    pid_e = Gtk.Entry()
    pid_e.set_text(pad_cfg.get("pid_file","") if pad_cfg else "")
    pid_e.set_placeholder_text("PID file (optional) — LED stays on while process lives")
    pid_e.set_activates_default(True)
    srow("PID file", pid_e)

    # Crosshair integration -- this invokes the installed CLI directly,
    # without a shell, and flips the overlay's current enabled state.
    crosshair_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
    box.pack_start(crosshair_box, False, False, 0)
    crosshair_hint = Gtk.Label(
        label="Pressing this pad runs crosshair --toggle to switch the overlay on/off."
    )
    crosshair_hint.set_xalign(0)
    crosshair_hint.set_line_wrap(True)
    crosshair_hint.get_style_context().add_class("lbl")
    crosshair_box.pack_start(crosshair_hint, False, False, 0)

    # stop-key section
    stop_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    box.pack_start(stop_box, False, False, 0)
    sk_c = Gtk.ComboBoxText()
    # Store the raw "page:note" key, never the display string. v2.2 saved
    # "0:17  (Airhorn)" and matched on it, so renaming the target pad raised
    # ValueError -> bare `except` -> set_active(0), silently repointing this
    # pad at whatever happened to be first in the list.
    sk_raw = [k.split("  (")[0] if "  (" in k else k for k in all_pad_keys]
    for k in all_pad_keys: sk_c.append_text(k)
    have_targets = all_pad_keys != ["(none)"]
    cur_sk  = (pad_cfg.get("stop_key","") if pad_cfg else "")
    cur_raw = cur_sk.split("  (")[0] if "  (" in cur_sk else cur_sk
    if cur_raw and cur_raw in sk_raw:
        sk_c.set_active(sk_raw.index(cur_raw))
    else:
        sk_c.set_active(0)
        if cur_raw: print(f"[pad] stop_key target {cur_raw!r} no longer exists")
    sk_c.set_sensitive(have_targets)
    row("Target pad", sk_c, stop_box)
    if not have_targets:
        _nt = Gtk.Label(label="Assign a sound or script pad first.")
        _nt.set_xalign(0); _nt.get_style_context().add_class("lbl")
        stop_box.pack_start(_nt, False, False, 0)

    def _show_hide(_=None):
        idx = act_c.get_active()
        play_box.set_visible(idx == 0)
        script_box.set_visible(idx == 1)
        stop_box.set_visible(idx == 4)
        crosshair_box.set_visible(idx == 5)

    act_c.connect("changed", _show_hide)
    _show_hide(); dlg.show_all(); _show_hide()

    resp = dlg.run()
    _test_cancel.set()
    test_worker = _test_thread[0]
    if test_worker is not None and test_worker is not threading.current_thread():
        test_worker.join(timeout=1.5)
    audio.stop(TK)

    def _color(): return color_names[col_c.get_active()] if col_c.get_active() >= 0 else "default"
    def _notes(): return notes_buf.get_text(notes_buf.get_start_iter(), notes_buf.get_end_iter(), False).strip()

    result = None
    if resp == Gtk.ResponseType.OK:
        idx = act_c.get_active()
        color = _color(); notes = _notes(); label = lbl_e.get_text().strip()
        if idx == 0:
            fname = file_btn.get_filename()
            if fname:
                result = {"action":"play","file":fname,"sink":get_sink(),
                          "vol":int(vol_adj.get_value()),"loop":loop_sw.get_active(),
                          "label":label,"retrigger":["restart","toggle","global"][rt_c.get_active()],
                          "color":color,"notes":notes}
        elif idx == 1:
            cmd = cmd_e.get_text().strip()
            if cmd:
                result = {"action":"script","cmd":cmd,"pid_file":pid_e.get_text().strip(),
                          "label":label or Path(cmd).stem,"color":color,"notes":notes}
        elif idx == 2:
            result = {"action":"stop_sfx","label":label or "Stop SFX","color":color,"notes":notes}
        elif idx == 3:
            result = {"action":"stop_all","label":label or "Stop All","color":color,"notes":notes}
        elif idx == 4:
            _ai  = sk_c.get_active()
            _tgt = sk_raw[_ai] if (have_targets and 0 <= _ai < len(sk_raw)) else ""
            if _tgt:
                result = {"action":"stop_key","stop_key":_tgt,
                          "label":label or "Stop","color":color,"notes":notes}
        elif idx == 5:
            result = {"action":"crosshair_toggle",
                      "label":label or "Crosshair","color":color,"notes":notes}

    dlg.destroy()
    return result


# settings dialog -- global options only

def settings_dialog(parent, cfg, sinks):
    dlg = Gtk.Dialog(title="Settings", transient_for=parent, modal=True)
    dlg.set_default_size(380, -1)
    dlg.add_buttons("Cancel", Gtk.ResponseType.CANCEL, "Save", Gtk.ResponseType.OK)
    dlg.get_widget_for_response(Gtk.ResponseType.OK).get_style_context().add_class("btn-ok")

    box = dlg.get_content_area()
    box.set_spacing(10); box.set_margin_start(14); box.set_margin_end(14)
    box.set_margin_top(10); box.set_margin_bottom(10)

    def row(lbl, w):
        hb = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        lb = Gtk.Label(label=lbl); lb.set_xalign(0); lb.set_width_chars(18)
        lb.get_style_context().add_class("lbl")
        hb.pack_start(lb, False, False, 0); hb.pack_start(w, True, True, 0)
        box.pack_start(hb, False, False, 0)

    # default sink
    sink_c = Gtk.ComboBoxText()
    _priority = ("vm_", "astro", "scarlett", "b1", "b2", "laptop", "loopback")
    virt = [s for s in sinks if any(k in s.lower() for k in _priority)]
    hw   = [s for s in sinks if s not in virt]
    combo_sinks = []
    for s in virt:
        sink_c.append_text(s); combo_sinks.append(s)
    if hw:
        sink_c.append_text("-- hardware --"); combo_sinks.append(None)
        for s in hw:
            sink_c.append_text(s); combo_sinks.append(s)
    cur = cfg.get("default_sink", "vm_game")
    try: sink_c.set_active(combo_sinks.index(cur))
    except ValueError: sink_c.set_active(next((i for i, v in enumerate(combo_sinks) if v), 0))
    row("Default output", sink_c)

    def get_sink():
        idx = sink_c.get_active()
        if 0 <= idx < len(combo_sinks) and combo_sinks[idx]: return combo_sinks[idx]
        return "vm_game"

    sop_sw = Gtk.Switch(); sop_sw.set_active(cfg.get("stop_on_page_change", False))
    sb = Gtk.Box(); sb.pack_start(sop_sw, False, False, 0)
    row("Stop on page change", sb)

    rt_c = Gtk.ComboBoxText()
    rt_c.append_text("Restart from start"); rt_c.append_text("Stop on 2nd press")
    rt_c.set_active(0 if cfg.get("retrigger","restart") == "restart" else 1)
    row("2nd press (default)", rt_c)

    # monitor section
    sep = Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL); sep.set_margin_top(4); sep.set_margin_bottom(2)
    box.pack_start(sep, False, False, 0)
    mon_title = Gtk.Label(label="Monitor / hear what you're sending")
    mon_title.set_xalign(0); mon_title.get_style_context().add_class("lbl")
    box.pack_start(mon_title, False, False, 0)

    mon_sw = Gtk.Switch(); mon_sw.set_active(cfg.get("monitor_enabled", False))
    msb = Gtk.Box(); msb.pack_start(mon_sw, False, False, 0)
    row("Monitor on", msb)

    mon_c = Gtk.ComboBoxText()
    for s in sinks: mon_c.append_text(s)
    cur_mon = cfg.get("monitor_sink", "")
    if cur_mon in sinks: mon_c.set_active(sinks.index(cur_mon))
    elif sinks: mon_c.set_active(0)
    row("Monitor output", mon_c)

    hint = Gtk.Label()
    hint.set_markup("<small>When on, sound plays on both the pad's output and this sink —\nso you hear what's being sent (e.g. to B1 / mic).</small>")
    hint.set_xalign(0); hint.set_line_wrap(True); hint.get_style_context().add_class("lbl")
    box.pack_start(hint, False, False, 0)

    dlg.show_all()
    resp = dlg.run()
    result = None
    if resp == Gtk.ResponseType.OK:
        result = dict(cfg)
        result["default_sink"]        = get_sink()
        result["stop_on_page_change"] = sop_sw.get_active()
        result["retrigger"]           = "restart" if rt_c.get_active() == 0 else "toggle"
        result["monitor_enabled"]     = mon_sw.get_active()
        result["monitor_sink"]        = mon_c.get_active_text() or ""
    dlg.destroy()
    return result


# main app

class PadfireApp:
    def __init__(self):
        self.cfg  = load_config()
        self._page = 0
        self.audio = AudioEngine()
        self.midi  = None

        self._win:      Gtk.Window | None = None
        self._midi_lbl: Gtk.Label  | None = None
        self._rac_lbl:  Gtk.Label  | None = None
        self._mon_btn:  Gtk.ToggleButton | None = None
        self._rac_vols: dict = {}
        self._pad_btns: dict = {}
        self._pad_css:  dict = {}
        self._page_btns: list = []
        self._rac_online  = False
        self._ipc_ref     = []
        self._led_dirty   = True
        self._prev_leds   = {}
        self._vu_level    = 0.0
        self._vu_ok       = True
        self._script_procs: dict = {}
        self._script_generations: dict[str, int] = {}
        self._script_threads: set[threading.Thread] = set()
        self._script_reapers: set[threading.Thread] = set()
        self._proc_lock   = threading.Lock()
        self._shutdown_event = threading.Event()
        self._rac_thread: threading.Thread | None = None
        self._vu_thread: threading.Thread | None = None
        self._pad_labels: dict = {}
        self._pad_state:  dict = {}
        self._file_ok:    dict = {}
        self._file_ok_t   = 0.0
        self._indicator   = None
        self._status_msg_id = 0

        self._build_ui()
        self._start_midi()
        self._start_ipc()
        self._start_rac_probe()
        self._start_vu()
        GLib.timeout_add(200, self._tick)

    # helpers

    def _note_for(self, row, col):
        return NOTE_SIDE(row) if col == 8 else NOTE_XY(row, col)

    def _pad_key(self, row, col):
        return f"{self._page}:{self._note_for(row, col)}"

    def _get_pad(self, row, col):
        return self.cfg["pages"][self._page].get(str(self._note_for(row, col)))

    def _set_pad(self, row, col, data):
        ns = str(self._note_for(row, col))
        if data: self.cfg["pages"][self._page][ns] = data
        else:    self.cfg["pages"][self._page].pop(ns, None)
        save_config(self.cfg)
        self._led_dirty = True
        GLib.idle_add(self._refresh_all)

    def _page_has(self, p): return bool(self.cfg["pages"][p])

    def _file_exists(self, path):
        # 72 pads x 5 refreshes/sec would be 360 stat() calls a second, so
        # results are cached and the cache is dropped at most every 2s
        if not path: return False
        now = time.monotonic()
        if now - self._file_ok_t > 2.0:
            self._file_ok.clear(); self._file_ok_t = now
        ok = self._file_ok.get(path)
        if ok is None:
            try: ok = Path(path).is_file()
            except Exception: ok = False
            self._file_ok[path] = ok
        return ok

    def _flash_status(self, text, warn=True):
        # v2.2 only printed failures to stdout, so from inside the app a broken
        # pad simply did nothing with no explanation
        lbl = getattr(self, "_msg_lbl", None)
        if lbl is None:
            print(f"[padfire] {text}"); return False
        lbl.set_text(text)
        ctx = lbl.get_style_context()
        ctx.remove_class("stat-dim"); ctx.remove_class("stat-warn")
        ctx.add_class("stat-warn" if warn else "stat-dim")
        lbl.show()
        self._status_msg_id += 1
        mine = self._status_msg_id
        def _clear():
            if mine == self._status_msg_id:
                cur = getattr(self, "_msg_lbl", None)
                if cur is not None: cur.set_text(""); cur.hide()
            return False
        GLib.timeout_add(6000, _clear)
        return False

    def _all_pad_keys(self):
        keys = []
        for p, page in enumerate(self.cfg["pages"]):
            for note_s, pad in page.items():
                action = pad.get("action","play")
                if action == "play" and pad.get("file"):
                    label = pad.get("label") or Path(pad.get("file", note_s)).stem
                    keys.append(f"{p}:{note_s}  ({label})")
                elif action == "script" and pad.get("cmd"):
                    label = pad.get("label") or Path(pad.get("cmd", note_s).split()[0]).stem
                    keys.append(f"{p}:{note_s}  ({label})")
        return keys or ["(none)"]

    # MIDI callbacks

    def _on_pad(self, row, col, pressed):
        if pressed: GLib.idle_add(self._trigger_pad, row, col)
        else:       GLib.idle_add(self._unflash, row, col)

    def _on_top(self, idx, pressed):
        if pressed: GLib.idle_add(self._switch_page, idx)

    def _on_side(self, row, pressed):
        if pressed: GLib.idle_add(self._trigger_pad, row, 8)
        else:       GLib.idle_add(self._unflash, row, 8)

    def _on_connect(self):
        self._prev_leds = {}; self._led_dirty = True
        GLib.idle_add(self._refresh_all); GLib.idle_add(self._update_midi_lbl)

    def _on_disconnect(self): GLib.idle_add(self._update_midi_lbl)

    # script state

    def _script_on(self, key, pad):
        with self._proc_lock:
            proc = self._script_procs.get(key)
        if proc is not None:
            if proc.poll() is None:
                return True
            with self._proc_lock:
                if self._script_procs.get(key) is proc:
                    self._script_procs.pop(key, None)
        pid_file = pad.get("pid_file", "").strip()
        if not pid_file:
            return False
        try:
            pp = Path(pid_file)
            if not pp.exists():
                return False
            for pid_s in pp.read_text().split():
                try:
                    os.kill(int(pid_s), 0)
                    return True
                except (ProcessLookupError, ValueError, OSError):
                    pass
        except Exception:
            pass
        return False

    @staticmethod
    def _reap_script_processes(processes):
        for proc in processes:
            _terminate_process(proc, timeout=0.5, process_group=True)

    def _reap_script_processes_async(self, processes):
        try:
            self._reap_script_processes(processes)
        finally:
            with self._proc_lock:
                self._script_reapers.discard(threading.current_thread())

    def _stop_script_processes(self, keys=None, *, wait=True):
        """Invalidate pending launches and stop every tracked script process."""
        processes = []
        tokens = {}
        with self._proc_lock:
            targets = (
                set(self._script_generations) | set(self._script_procs)
                if keys is None else set(keys)
            )
            for key in targets:
                generation = self._script_generations.get(key, 0) + 1
                self._script_generations[key] = generation
                tokens[key] = generation
                proc = self._script_procs.pop(key, None)
                if proc is not None:
                    processes.append(proc)
        # Signal all children first so several scripts do not each consume a
        # full timeout serially; then reap/force-kill any that ignore SIGTERM.
        for proc in processes:
            try:
                if proc.poll() is None:
                    _signal_process(proc, signal.SIGTERM, process_group=True)
            except Exception:
                pass
        if processes:
            if wait:
                self._reap_script_processes(processes)
            else:
                reaper = threading.Thread(
                    target=self._reap_script_processes_async,
                    args=(processes,),
                    daemon=True,
                    name="padfire-script-reaper",
                )
                with self._proc_lock:
                    self._script_reapers.add(reaper)
                    try:
                        reaper.start()
                    except Exception:
                        self._script_reapers.discard(reaper)
                        self._reap_script_processes(processes)
        return tokens

    def _stop_key(self, key):
        self.audio.stop(key)
        self._stop_script_processes((key,))
        self._led_dirty = True

    # pad trigger

    def _trigger_pad(self, row, col):
        shutdown_event = getattr(self, "_shutdown_event", None)
        if shutdown_event is not None and shutdown_event.is_set():
            return False
        self._flash(row, col)
        pad = self._get_pad(row, col)
        if not pad: return False

        action = pad.get("action","play")
        key    = self._pad_key(row, col)
        print(f"[pad] R{row}C{col} key={key} action={action}")

        if action == "stop_sfx":
            threading.Thread(target=self.audio.stop_all, daemon=True).start()
            self._led_dirty = True; return False

        if action == "stop_all":
            # Invalidate script starts synchronously in the callback; process
            # reaping and audio stop can finish on workers without a new script
            # slipping in between the pad press and the worker starting.
            self._stop_script_processes(wait=False)

            def _all():
                self.audio.stop_all()
                self._led_dirty = True

            threading.Thread(target=_all, daemon=True).start()
            self._led_dirty = True; return False

        if action == "stop_key":
            raw = pad.get("stop_key","")
            tkey = raw.split("  (")[0] if "  (" in raw else raw
            threading.Thread(target=lambda: self._stop_key(tkey), daemon=True).start()
            self._led_dirty = True; return False

        if action == "crosshair_toggle":
            def _toggle_crosshair():
                command = crosshair_toggle_argv()
                if not command:
                    GLib.idle_add(self._flash_status, "Crosshair CLI not found")
                    return
                try:
                    result = subprocess.run(
                        command, capture_output=True, text=True, timeout=5, check=False
                    )
                    if result.returncode != 0:
                        detail = (result.stderr or result.stdout).strip()
                        message = detail or f"exit status {result.returncode}"
                        GLib.idle_add(
                            self._flash_status, f"Crosshair toggle failed: {message[:100]}"
                        )
                    else:
                        print("[padfire] Crosshair toggled")
                except subprocess.TimeoutExpired:
                    GLib.idle_add(self._flash_status, "Crosshair toggle timed out")
                except OSError as exc:
                    GLib.idle_add(self._flash_status, f"Crosshair toggle failed: {exc}")

            threading.Thread(target=_toggle_crosshair, daemon=True).start()
            return False

        if action == "script":
            cmd = pad.get("cmd", "").strip()
            if not cmd:
                return False
            # A pad owns at most one script. Retriggering stops its previous
            # process and invalidates a launch that has not registered yet.
            tokens = self._stop_script_processes((key,), wait=False)
            generation = tokens[key]

            def _run():
                proc = None
                try:
                    with self._proc_lock:
                        if (
                            self._shutdown_event.is_set()
                            or self._script_generations.get(key) != generation
                        ):
                            return
                    proc = subprocess.Popen(
                        cmd, shell=True, stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT, text=True, bufsize=1,
                        start_new_session=(os.name == "posix"),
                    )
                    with self._proc_lock:
                        current = (
                            not self._shutdown_event.is_set()
                            and self._script_generations.get(key) == generation
                        )
                        if current:
                            self._script_procs[key] = proc
                    if not current:
                        _terminate_process(
                            proc, process_group=(os.name == "posix")
                        )
                        return
                    self._led_dirty = True
                    out, _ = proc.communicate(timeout=600)
                    if out:
                        for line in out.strip().splitlines()[:10]:
                            print(f"[script] {line}")
                except subprocess.TimeoutExpired:
                    print(f"[script] {key}: timeout")
                    if proc is not None:
                        _terminate_process(
                            proc, process_group=(os.name == "posix")
                        )
                except Exception as e:
                    print(f"[script] {key}: {e}")
                finally:
                    with self._proc_lock:
                        if proc is not None and self._script_procs.get(key) is proc:
                            self._script_procs.pop(key, None)
                        self._script_threads.discard(threading.current_thread())
                    self._led_dirty = True

            worker = threading.Thread(
                target=_run, daemon=True, name=f"script-{key}"
            )
            with self._proc_lock:
                if (
                    self._shutdown_event.is_set()
                    or self._script_generations.get(key) != generation
                ):
                    return False
                self._script_threads.add(worker)
                try:
                    worker.start()
                except Exception as exc:
                    self._script_threads.discard(worker)
                    print(f"[script] {key}: could not start worker: {exc}")
            return False

        # play
        if not pad.get("file"): return False
        if not self._file_exists(pad["file"]):
            # a moved or deleted file used to look identical to a working pad,
            # and pressing it only printed to stdout
            print(f"[pad] {key}: file not found: {pad['file']}")
            self._flash_status(f"Missing file: {Path(pad['file']).name}")
            self._led_dirty = True
            return False
        rt = pad.get("retrigger","global")
        if rt == "global": rt = self.cfg.get("retrigger","restart")
        if self.audio.is_playing(key) and rt == "toggle":
            threading.Thread(target=lambda: self.audio.stop(key), daemon=True).start()
            self._led_dirty = True; return False

        filepath = pad["file"]
        sink     = pad.get("sink", self.cfg["default_sink"])
        vol      = pad.get("vol", 80)
        loop     = pad.get("loop", False)
        mon      = self.cfg.get("monitor_sink") if self.cfg.get("monitor_enabled") else None

        def _bg():
            self.audio.stop(key)
            self.audio.play(key, filepath, sink, vol, loop, monitor_sink=mon)
            self._led_dirty = True
        threading.Thread(target=_bg, daemon=True).start()
        return False

    def _flash(self, row, col):
        btn = self._pad_btns.get((row, col))
        if not btn: return
        ctx = btn.get_style_context(); ctx.add_class("pad-flash")
        GLib.timeout_add(120, lambda: (ctx.remove_class("pad-flash"), False)[1])

    def _unflash(self, row, col):
        btn = self._pad_btns.get((row, col))
        if btn: btn.get_style_context().remove_class("pad-flash")
        return False

    def _switch_page(self, idx):
        if self.cfg.get("stop_on_page_change"):
            threading.Thread(target=self.audio.stop_all, daemon=True).start()
        self._page = max(0, min(NUM_PAGES-1, idx))
        self._prev_leds = {}
        self._led_dirty = True; self._refresh_all(); self._update_tray()
        return False

    # LED

    def _led_for(self, pg, page, note):
        key = f"{page}:{note}"
        pad = pg.get(str(note))
        if not pad: return LED_OFF
        action = pad.get("action","play")
        clr    = PAD_COLORS.get(pad.get("color","default"), PAD_COLORS["default"])[0]
        if action in ("stop_sfx","stop_all","stop_key"):
            return clr if clr else LED_RED
        if action == "crosshair_toggle":
            return clr if clr else LED_AMBER
        if action == "script":
            return LED_GREEN if self._script_on(key, pad) else (clr if clr else LED_ORANGE)
        if self.audio.is_playing(key): return LED_GREEN
        return clr if clr else LED_DIM

    def _refresh_leds(self):
        if not self.midi or not self.midi.is_connected() or not self._led_dirty: return
        self._led_dirty = False
        pg = self.cfg["pages"][self._page]
        want = {}
        for r in range(NUM_ROWS):
            for c in range(NUM_COLS):
                n = NOTE_XY(r, c); want[f"n:{n}"] = self._led_for(pg, self._page, n)
            n = NOTE_SIDE(r); want[f"n:{n}"] = self._led_for(pg, self._page, n)
        for i, cc in enumerate(CC_TOP):
            want[f"c:{cc}"] = LED_AMBER if i==self._page else (LED_PG_HAS if self._page_has(i) else LED_OFF)
        for k, color in want.items():
            if self._prev_leds.get(k) != color:
                if k.startswith("n:"): self.midi.led(int(k[2:]), color)
                else:                  self.midi.cc_led(int(k[2:]), color)
        self._prev_leds = want

    # GTK grid

    def _apply_color(self, row, col, btn, color_key):
        k = (row, col)
        color_key = color_key or "default"
        cur = self._pad_css.get(k)
        # skip the costly provider rebuild when the color is unchanged
        # (this runs for every pad ~5x/sec from the refresh tick)
        if cur and cur[0] == color_key: return
        if cur:
            btn.get_style_context().remove_provider(cur[1]); self._pad_css.pop(k, None)
        if color_key == "default": return
        entry = PAD_COLORS.get(color_key)
        if not entry or not entry[1]: return
        _, bg, border, fg = entry
        css = f"button.pad-btn {{ background:{bg}; border-color:{border}; color:{fg}; }}\n"
        prov = Gtk.CssProvider(); prov.load_from_data(css.encode())
        btn.get_style_context().add_provider(prov, Gtk.STYLE_PROVIDER_PRIORITY_USER)
        self._pad_css[k] = (color_key, prov)

    def _pad_text(self, row, col, text):
        lab = self._pad_labels.get((row, col))
        if lab is not None:
            lab.set_text(text)
        else:
            btn = self._pad_btns.get((row, col))
            if btn is not None: btn.set_label(text)

    def _pad_state_for(self, pg, note, key):
        """Everything that affects how a pad looks, as a comparable tuple.
        The page number is included so switching pages invalidates every
        cached entry."""
        pad = pg.get(str(note))
        if not pad: return (self._page, "empty")
        action = pad.get("action", "play")
        if action == "crosshair_toggle":
            return (self._page, "crosshair", pad.get("label",""),
                    pad.get("color"), pad.get("notes",""))
        if action in ("stop_sfx", "stop_all"):
            return (self._page, "stop", action, pad.get("label",""), pad.get("color"))
        if action == "script":
            return (self._page, "script", pad.get("label",""), pad.get("cmd",""),
                    pad.get("color"), self._script_on(key, pad), pad.get("notes",""))
        if action == "stop_key":
            return (self._page, "stopkey", pad.get("label",""),
                    pad.get("stop_key",""), pad.get("color"))
        return (self._page, "play", pad.get("label",""), pad.get("file",""),
                pad.get("color"), self.audio.is_playing(key),
                self._file_exists(pad.get("file")), pad.get("vol",80),
                pad.get("notes",""), bool(pad.get("loop")))

    def _refresh_grid(self):
        pg = self.cfg["pages"][self._page]
        for (row, col), btn in self._pad_btns.items():
            note = self._note_for(row, col)
            key  = f"{self._page}:{note}"
            pad  = pg.get(str(note))

            # Skip pads whose appearance cannot have changed. v2.2 rewrote the
            # label, tooltip and style classes of all 72 pads 5x a second
            # (~360 widget updates/sec), which made tooltips flicker away while
            # you were still reading them.
            state = self._pad_state_for(pg, note, key)
            if self._pad_state.get((row, col)) == state: continue
            self._pad_state[(row, col)] = state

            ctx  = btn.get_style_context()
            for cls in ("pad-has","pad-playing","pad-stop","pad-script",
                        "pad-script-on","pad-missing","pad-action"):
                ctx.remove_class(cls)
            self._apply_color(row, col, btn, pad.get("color") if pad else None)

            if not pad:
                self._pad_text(row, col, ""); btn.set_tooltip_text(None); continue

            action = pad.get("action","play")
            lbl    = pad.get("label","")
            plain  = not pad.get("color") or pad.get("color") == "default"

            if action in ("stop_sfx","stop_all"):
                self._pad_text(row, col, lbl or ("Stop All" if action=="stop_all"
                                                 else "Stop SFX"))
                if plain: ctx.add_class("pad-stop")
                btn.set_tooltip_text(
                    "Stop all sounds and scripts" if action=="stop_all"
                    else "Stop audio only (scripts keep running)")

            elif action == "crosshair_toggle":
                self._pad_text(row, col, lbl or "Crosshair")
                if plain: ctx.add_class("pad-action")
                notes = pad.get("notes","")
                tip = "Toggle Crosshair overlay on/off"
                if notes: tip += f"\n{notes[:160]}"
                tip += "\n\nRight-click to edit"
                btn.set_tooltip_text(tip)

            elif action == "script":
                on = self._script_on(key, pad)
                self._pad_text(row, col, lbl or Path(pad.get("cmd","?")).stem)
                if on:      ctx.add_class("pad-script-on")
                elif plain: ctx.add_class("pad-script")
                notes = pad.get("notes","")
                tip = f"{'Running' if on else 'Idle'}: {pad.get('cmd','')[:80]}"
                if notes: tip += f"\n{notes[:160]}"
                tip += "\n\nRight-click to edit"
                btn.set_tooltip_text(tip)

            elif action == "stop_key":
                self._pad_text(row, col, lbl or "Stop")
                if plain: ctx.add_class("pad-stop")
                _tgt = pad.get("stop_key","")
                btn.set_tooltip_text(
                    f"Stops pad {_tgt}" if _tgt
                    else "No target pad set -- right-click to edit")

            else:  # play
                fpath   = pad.get("file","")
                stem    = lbl or (Path(fpath).stem if fpath else "")
                missing = bool(fpath) and not self._file_exists(fpath)
                playing = self.audio.is_playing(key)
                # full name in a wrapping 2-line label instead of a hard 9-char
                # chop, which made "clicker settings" and "clicker toggle"
                # render as the near-identical "clicker s" / "clicker t"
                self._pad_text(row, col, ("⚠ " + stem) if missing else stem)
                if missing:   ctx.add_class("pad-missing")
                elif playing: ctx.add_class("pad-playing")
                elif plain:   ctx.add_class("pad-has")
                if missing:
                    btn.set_tooltip_text(
                        f"FILE NOT FOUND\n{fpath}\n\nRight-click to pick it again")
                else:
                    notes = pad.get("notes","")
                    tip = f"{Path(fpath).name if fpath else '?'}  {pad.get('vol',80)}%"
                    if pad.get("loop"): tip += "  (loops)"
                    if notes: tip += f"\n{notes[:160]}"
                    tip += "\n\nRight-click to edit"
                    btn.set_tooltip_text(tip)

    def _refresh_pages(self):
        for i, btn in enumerate(self._page_btns):
            ctx = btn.get_style_context()
            ctx.remove_class("page-active"); ctx.remove_class("page-has")
            if i == self._page:             ctx.add_class("page-active")
            elif self._page_has(i):         ctx.add_class("page-has")

    def _update_midi_lbl(self):
        conn = self.midi and self.midi.is_connected()
        if self._midi_lbl:
            self._midi_lbl.set_text("● MIDI" if conn else "○ MIDI")
            ctx = self._midi_lbl.get_style_context()
            ctx.remove_class("midi-ok"); ctx.remove_class("midi-bad")
            ctx.add_class("midi-ok" if conn else "midi-bad")
        return False

    def _refresh_all(self):
        self._refresh_grid(); self._refresh_pages(); self._refresh_leds()
        return False

    def _tick(self):
        if self._led_dirty: self._refresh_leds()
        self._refresh_grid()
        if getattr(self, "_vu_bar", None) is not None:
            self._vu_bar.set_fraction(self._vu_level)
        self._update_playing_lbl()
        return True

    def _update_playing_lbl(self):
        # "what is making noise right now" was only ever visible on the
        # Launchpad LEDs; the window itself never said
        lbl = getattr(self, "_play_lbl", None)
        if lbl is None: return
        with self.audio._lock:
            keys = [k for k in self.audio._procs if not k.startswith("__mon__")]
        if not keys:
            if lbl.get_text(): lbl.set_text("")
            return
        names = []
        for k in keys[:2]:
            try:
                p_s, n_s = k.split(":", 1)
                pad = self.cfg["pages"][int(p_s)].get(n_s, {})
                names.append(pad.get("label")
                             or (Path(pad["file"]).stem if pad.get("file") else k))
            except Exception:
                names.append(k)
        txt = "▶ " + ", ".join(names)
        if len(keys) > 2: txt += f"  +{len(keys)-2}"
        if lbl.get_text() != txt: lbl.set_text(txt)

    # RAC probe

    def _start_rac_probe(self):
        def probe():
            while not self._shutdown_event.is_set():
                raw = rac_send("get_sinks")
                if self._shutdown_event.is_set():
                    break
                try:
                    data = json.loads(raw) if raw else None
                except Exception as e:
                    # a malformed reply used to be indistinguishable from
                    # "Hearth is not running"
                    print(f"[rac] bad reply: {e}")
                    data = None
                self._rac_online = data is not None
                GLib.idle_add(self._update_rac, data)
                if self._shutdown_event.wait(3):
                    break

        self._rac_thread = threading.Thread(
            target=probe, daemon=True, name="rac-probe"
        )
        self._rac_thread.start()

    # VU monitor

    def _vu_source(self):
        """Monitor source for the level meter, from config.

        v2.2 hardcoded "vm_game.monitor", so the meter was permanently dead
        for anyone whose sinks are named anything else.
        """
        sink = (self.cfg.get("monitor_sink") if self.cfg.get("monitor_enabled")
                else None) or self.cfg.get("default_sink") or ""
        sink = str(sink).strip()
        if not sink: return None
        return sink if sink.endswith(".monitor") else sink + ".monitor"

    def _hide_vu(self):
        if getattr(self, "_vu_bar", None) is not None: self._vu_bar.hide()
        return False

    def _start_vu(self):
        if not shutil.which("parec"):
            # without this guard Popen raised every 50ms while anything played
            self._vu_ok = False
            print("[vu] parec not found -- level meter disabled")
            GLib.idle_add(self._hide_vu)
            return

        def _mon():
            RATE = 8000
            proc = None
            src = None
            backoff = 0.0
            pending = b""

            def stop_capture():
                nonlocal proc, src, pending
                current = proc
                proc = None
                src = None
                pending = b""
                if current is None:
                    return
                _terminate_process(current)
                pipe = getattr(current, "stdout", None)
                if pipe is not None:
                    try:
                        pipe.close()
                    except Exception:
                        pass

            try:
                while not self._shutdown_event.is_set():
                    with self.audio._lock:
                        playing = bool(self.audio._procs)
                    want = self._vu_source()
                    # Tear down when idle or when the configured sink changes.
                    if proc is not None and (not playing or want != src):
                        stop_capture()
                        self._vu_level = 0.0
                    if (
                        playing and proc is None and want
                        and time.monotonic() >= backoff
                    ):
                        try:
                            proc = subprocess.Popen(
                                ["parec", f"--device={want}", "--channels=1",
                                 f"--rate={RATE}", "--format=s16le", "--raw",
                                 "--latency-msec=33"],
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                bufsize=0,
                            )
                            src = want
                            pending = b""
                        except Exception as e:
                            proc = None
                            src = None
                            backoff = time.monotonic() + 5.0
                            print(f"[vu] parec failed ({e}) -- retrying in 5s")
                    if proc is not None and proc.poll() is not None:
                        # Reap exited processes and close their pipe before retry.
                        stop_capture()
                        self._vu_level = 0.0
                        backoff = time.monotonic() + 5.0
                    if proc is not None:
                        try:
                            ready, _, _ = select.select([proc.stdout], [], [], 0.05)
                            if ready:
                                raw = proc.stdout.read(2048)
                                if not raw:
                                    stop_capture()
                                    self._vu_level = 0.0
                                    backoff = time.monotonic() + 5.0
                                    continue
                                # Keep an odd trailing byte for the next read:
                                # PCM s16le samples must never lose alignment.
                                data = pending + raw
                                if len(data) % 2:
                                    pending = data[-1:]
                                    data = data[:-1]
                                else:
                                    pending = b""
                                count = len(data) // 2
                                if count:
                                    samples = struct.unpack(f"<{count}h", data)
                                    rms = math.sqrt(sum(s * s for s in samples) / count) / 32768.0
                                    db = 20 * math.log10(rms) if rms > 1e-7 else -96
                                    self._vu_level = max(0.0, min(1.0, (db + 60) / 60))
                        except Exception as e:
                            print(f"[vu] capture read failed: {e}")
                            stop_capture()
                            self._vu_level = 0.0
                            backoff = time.monotonic() + 5.0
                    else:
                        self._vu_level = 0.0
                        self._shutdown_event.wait(0.05)
            finally:
                stop_capture()
                self._vu_level = 0.0

        self._vu_thread = threading.Thread(
            target=_mon, daemon=True, name="padfire-vu"
        )
        self._vu_thread.start()

    # IPC

    def _start_ipc(self):
        def handler(cmd):
            if cmd == "quit":    GLib.idle_add(self._quit); return "ok"
            if cmd == "show":    GLib.idle_add(self._show); return "ok"
            if cmd == "reload":
                self.cfg = load_config(); self._led_dirty = True
                GLib.idle_add(self._refresh_all); return "ok"
            if cmd == "stop_all":
                threading.Thread(target=self.audio.stop_all, daemon=True).start(); return "ok"
            if cmd == "status":
                with self.audio._lock:
                    pkeys = [k for k in self.audio._procs if not k.startswith("__mon__")]
                pinfo = []
                for k in pkeys:
                    try:
                        pg_s, note_s = k.split(":",1)
                        pad = self.cfg["pages"][int(pg_s)].get(note_s, {})
                        pinfo.append({"key":k,"label":pad.get("label") or
                                      (Path(pad["file"]).stem if pad.get("file") else k),
                                      "sink":pad.get("sink",""),"page":int(pg_s)})
                    except Exception: pinfo.append({"key":k,"label":k,"sink":"","page":0})
                return json.dumps({"connected":self.midi.is_connected() if self.midi else False,
                                   "page":self._page,"playing":pkeys,"playing_info":pinfo,
                                   "device":DEVICE_SUBSTR,"version":VER,
                                   "monitor_enabled":self.cfg.get("monitor_enabled",False),
                                   "monitor_sink":self.cfg.get("monitor_sink","")})
            return "unknown"
        threading.Thread(target=ipc_serve, args=(handler, self._ipc_ref), daemon=True).start()

    def _start_midi(self):
        self.midi = MidiEngine(self._on_pad, self._on_top, self._on_side,
                               self._on_connect, self._on_disconnect)
        threading.Thread(target=self.midi.try_connect, daemon=True).start()

    # UI build

    def _build_ui(self):
        screen = Gdk.Screen.get_default()
        if screen is None:
            # v2.2 died here with the unhelpful GObject error
            # "Argument 0 does not allow None as a value"
            print("[padfire] cannot open a display.\n"
                  "  Set DISPLAY or WAYLAND_DISPLAY, or start Padfire from "
                  "inside your desktop session.", file=sys.stderr)
            raise SystemExit(1)
        prov = Gtk.CssProvider(); prov.load_from_data(CSS.encode())
        Gtk.StyleContext.add_provider_for_screen(screen, prov,
                                                  Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        Gtk.Settings.get_default().set_property("gtk-application-prefer-dark-theme", True)

        win = Gtk.Window(title=f"Padfire {VER}")
        win.set_resizable(True); win.set_default_size(-1, -1)
        # without an icon name the taskbar/alt-tab entry was a generic square
        try:
            _theme = Gtk.IconTheme.get_default()
            for _nm in ("padfire", "audio-x-generic"):
                if _theme.has_icon(_nm):
                    win.set_icon_name(_nm)
                    Gtk.Window.set_default_icon_name(_nm)
                    break
        except Exception: pass
        win.connect("delete-event", self._on_close)
        win.connect("key-press-event", self._on_key)
        self._win = win
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        win.add(root)

        # top bar -- logo, MIDI status, stop, monitor, settings
        topbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        topbar.get_style_context().add_class("topbar")
        logo = Gtk.Label(label="PADFIRE"); logo.get_style_context().add_class("app-logo")
        topbar.pack_start(logo, False, False, 0)
        topbar.pack_start(Gtk.Box(), True, True, 0)

        self._midi_lbl = Gtk.Label(label="○ MIDI"); self._midi_lbl.get_style_context().add_class("midi-bad")
        topbar.pack_start(self._midi_lbl, False, False, 0)

        btn_stop = Gtk.Button(label="Stop")
        btn_stop.get_style_context().add_class("btn-stop")
        btn_stop.set_tooltip_text("Stop all sounds")
        btn_stop.connect("clicked", lambda *_: threading.Thread(target=self.audio.stop_all, daemon=True).start())
        topbar.pack_start(btn_stop, False, False, 4)

        self._mon_btn = Gtk.ToggleButton(label="Mon")
        self._mon_btn.get_style_context().add_class("act")
        self._mon_btn.set_tooltip_text("Monitor: also play sounds in your headset\nRight-click for settings")
        self._mon_btn.connect("toggled", self._on_mon_toggled)
        def _mon_rc(w, ev):
            if ev.button == 3: self._open_settings(); return True
        self._mon_btn.connect("button-press-event", _mon_rc)
        topbar.pack_start(self._mon_btn, False, False, 2)

        cfg_btn = Gtk.Button(label="⚙"); cfg_btn.get_style_context().add_class("act")
        cfg_btn.set_tooltip_text("Settings"); cfg_btn.connect("clicked", self._open_settings)
        topbar.pack_start(cfg_btn, False, False, 0)
        root.pack_start(topbar, False, False, 0)

        # grid
        wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        wrap.get_style_context().add_class("grid-wrap")
        wrap.set_halign(Gtk.Align.CENTER); wrap.set_hexpand(True)

        grid = Gtk.Grid(); grid.set_row_spacing(2); grid.set_column_spacing(2)
        grid.set_hexpand(False); grid.set_vexpand(False)

        # page row (top)
        for i in range(NUM_PAGES):
            btn = Gtk.Button(label=str(i+1))
            btn.get_style_context().add_class("page-btn")
            btn.set_size_request(PAD_PX, 20)
            btn.set_hexpand(False); btn.set_vexpand(False)
            btn.set_tooltip_text(f"Page {i+1}")
            btn.connect("clicked", lambda _, p=i: self._switch_page(p))
            grid.attach(btn, i, 0, 1, 1)
            self._page_btns.append(btn)
        corner = Gtk.Box(); corner.set_size_request(30, 20)
        grid.attach(corner, NUM_COLS, 0, 1, 1)

        # pads + side column
        for row in range(NUM_ROWS):
            for col in range(NUM_COLS):
                btn = Gtk.Button()
                btn.get_style_context().add_class("pad-btn")
                btn.set_size_request(PAD_PX, PAD_PX)
                btn.set_hexpand(False); btn.set_vexpand(False)
                # A wrapping child label instead of plain button text, so names
                # use two lines and ellipsize properly rather than being cut to
                # 9 characters.
                _lab = Gtk.Label(label="")
                _lab.set_line_wrap(True)
                _lab.set_line_wrap_mode(Pango.WrapMode.WORD_CHAR)
                _lab.set_lines(2)
                _lab.set_ellipsize(Pango.EllipsizeMode.END)
                _lab.set_justify(Gtk.Justification.CENTER)
                _lab.set_max_width_chars(1)
                btn.add(_lab)
                self._pad_labels[(row, col)] = _lab
                r, c = row, col
                btn.connect("clicked", lambda _, ro=r, co=c: self._on_click(ro, co))
                def _rc(w, ev, ro=r, co=c):
                    if ev.button == 3: self._ctx_menu(w, ev, ro, co); return True
                btn.connect("button-press-event", _rc)
                # file drop
                btn.drag_dest_set(Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY | Gdk.DragAction.MOVE)
                btn.drag_dest_add_uri_targets(); btn.drag_dest_add_text_targets()
                btn.connect("drag-data-received", lambda w,ctx,x,y,d,i,t,ro=r,co=c: self._on_drop(w,ctx,x,y,d,i,t,ro,co))
                # pad drag
                btn.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.MOVE)
                btn.drag_source_add_text_targets()
                btn.connect("drag-begin",     lambda w,ctx,ro=r,co=c: w.get_style_context().add_class("pad-flash"))
                btn.connect("drag-data-get",  lambda w,ctx,d,i,t,ro=r,co=c: d.set_text(f"{ro},{co}",-1))
                btn.connect("drag-motion",    lambda w,ctx,x,y,t,ro=r,co=c: (w.get_style_context().add_class("pad-dragover"), Gdk.drag_status(ctx, Gdk.DragAction.MOVE, t), True)[2])
                btn.connect("drag-leave",     lambda w,ctx,t: w.get_style_context().remove_class("pad-dragover"))
                grid.attach(btn, col, row+1, 1, 1)
                self._pad_btns[(row, col)] = btn

            sbtn = Gtk.Button()
            sbtn.get_style_context().add_class("pad-btn"); sbtn.get_style_context().add_class("side-pad")
            sbtn.set_size_request(30, PAD_PX); sbtn.set_hexpand(False); sbtn.set_vexpand(False)
            _slab = Gtk.Label(label="")
            _slab.set_line_wrap(True)
            _slab.set_line_wrap_mode(Pango.WrapMode.WORD_CHAR)
            _slab.set_lines(2)
            _slab.set_ellipsize(Pango.EllipsizeMode.END)
            _slab.set_justify(Gtk.Justification.CENTER)
            _slab.set_max_width_chars(1)
            sbtn.add(_slab)
            self._pad_labels[(row, 8)] = _slab
            ro = row
            sbtn.connect("clicked", lambda _, ro=ro: self._on_click(ro, 8))
            def _src(w, ev, ro=ro):
                if ev.button == 3: self._ctx_menu(w, ev, ro, 8); return True
            sbtn.connect("button-press-event", _src)
            sbtn.drag_dest_set(Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY | Gdk.DragAction.MOVE)
            sbtn.drag_dest_add_uri_targets(); sbtn.drag_dest_add_text_targets()
            sbtn.connect("drag-data-received", lambda w,ctx,x,y,d,i,t,ro=ro: self._on_drop(w,ctx,x,y,d,i,t,ro,8))
            sbtn.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.MOVE)
            sbtn.drag_source_add_text_targets()
            sbtn.connect("drag-begin",     lambda w,ctx,ro=ro: w.get_style_context().add_class("pad-flash"))
            sbtn.connect("drag-data-get",  lambda w,ctx,d,i,t,ro=ro: d.set_text(f"{ro},8",-1))
            sbtn.connect("drag-motion",    lambda w,ctx,x,y,t,ro=ro: (w.get_style_context().add_class("pad-dragover"), Gdk.drag_status(ctx, Gdk.DragAction.MOVE, t), True)[2])
            sbtn.connect("drag-leave",     lambda w,ctx,t: w.get_style_context().remove_class("pad-dragover"))
            grid.attach(sbtn, NUM_COLS, row+1, 1, 1)
            self._pad_btns[(row, 8)] = sbtn

        wrap.pack_start(grid, False, False, 0)
        root.pack_start(wrap, True, False, 0)

        # status bar -- compact, only what matters
        status = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        status.get_style_context().add_class("statusbar")

        self._vu_bar = Gtk.ProgressBar(); self._vu_bar.set_name("vu")
        self._vu_bar.set_size_request(54, 4); self._vu_bar.set_valign(Gtk.Align.CENTER)
        status.pack_start(self._vu_bar, False, False, 0)

        self._rac_lbl = Gtk.Label(label="○ Hearth"); self._rac_lbl.get_style_context().add_class("stat-dim")
        status.pack_start(self._rac_lbl, False, False, 2)

        # hearth volumes -- only shown when connected, hidden otherwise
        self._rac_vol_box = Gtk.Box(spacing=5)
        for sink, tag in [("vm_game","G"),("vm_chat","C"),("vm_music","M")]:
            tl = Gtk.Label(label=tag); tl.get_style_context().add_class("stat-dim")
            self._rac_vol_box.pack_start(tl, False, False, 0)
            vl = Gtk.Label(label=""); vl.get_style_context().add_class("stat-val")
            self._rac_vol_box.pack_start(vl, False, False, 0)
            self._rac_vols[sink] = vl
        # set_visible(False) alone is undone by win.show_all(), so "G C M"
        # used to show with no values whenever Hearth was offline
        self._rac_vol_box.set_no_show_all(True)
        self._rac_vol_box.set_visible(False)
        status.pack_start(self._rac_vol_box, False, False, 0)

        # what is playing right now
        self._play_lbl = Gtk.Label(label=""); self._play_lbl.set_xalign(0)
        self._play_lbl.get_style_context().add_class("stat-play")
        self._play_lbl.set_ellipsize(Pango.EllipsizeMode.END)
        status.pack_start(self._play_lbl, True, True, 6)

        # transient errors (missing file, etc) instead of stdout-only
        self._msg_lbl = Gtk.Label(label=""); self._msg_lbl.set_xalign(1)
        self._msg_lbl.get_style_context().add_class("stat-warn")
        self._msg_lbl.set_ellipsize(Pango.EllipsizeMode.END)
        self._msg_lbl.set_no_show_all(True); self._msg_lbl.set_visible(False)
        status.pack_start(self._msg_lbl, False, False, 4)

        # The Hearth button only exists when Hearth does, and rac_send runs off
        # the main thread: it has a 1s socket timeout, which was enough to
        # visibly freeze the window when Hearth was not installed.
        self._hrth_btn = Gtk.Button(label="Hearth")
        self._hrth_btn.get_style_context().add_class("act")
        self._hrth_btn.get_style_context().add_class("btn-sm")
        self._hrth_btn.set_tooltip_text("Bring the Hearth window to the front")
        self._hrth_btn.connect("clicked", lambda *_: threading.Thread(
            target=lambda: rac_send("show"), daemon=True).start())
        self._hrth_btn.set_no_show_all(True); self._hrth_btn.set_visible(False)
        status.pack_end(self._hrth_btn, False, False, 0)
        root.pack_start(status, False, False, 0)

        self._setup_tray()
        self._refresh_all()
        win.show_all()
        self._win.resize(1, 1)

        # sync monitor button without firing the toggled handler
        self._mon_btn.handler_block_by_func(self._on_mon_toggled)
        self._mon_btn.set_active(self.cfg.get("monitor_enabled", False))
        self._mon_btn.handler_unblock_by_func(self._on_mon_toggled)
        self._update_mon_btn()

    # pad interactions

    def _on_click(self, row, col):
        pad = self._get_pad(row, col)
        if pad: self._trigger_pad(row, col)
        else:   self._edit(row, col)

    def _edit(self, row, col):
        result = pad_edit_dialog(self._win, self._get_pad(row, col),
                                 self.audio.list_sinks(refresh=True),
                                 self.cfg.get("default_sink","vm_game"),
                                 self.audio, self._all_pad_keys())
        if result is not None: self._set_pad(row, col, result)

    def _ctx_menu(self, widget, event, row, col):
        pad = self._get_pad(row, col); key = self._pad_key(row, col)
        menu = Gtk.Menu()
        def item(lbl, cb, on=True):
            it = Gtk.MenuItem(label=lbl); it.connect("activate", lambda *_: cb())
            it.set_sensitive(on); menu.append(it)
        item("Edit...",        lambda: self._edit(row, col))
        item("Trigger",        lambda: self._trigger_pad(row, col), on=bool(pad))
        item("Stop this pad",  lambda: threading.Thread(target=lambda: self._stop_key(key), daemon=True).start(),
             on=self.audio.is_playing(key) or (pad and pad.get("action") == "script" and self._script_on(key, pad)))
        menu.append(Gtk.SeparatorMenuItem())
        item("Clear pad",      lambda: self._clear(row, col), on=bool(pad))
        menu.show_all(); menu.popup_at_pointer(event)

    def _clear(self, row, col):
        threading.Thread(target=lambda: self._stop_key(self._pad_key(row, col)), daemon=True).start()
        self._set_pad(row, col, None)

    def _on_drop(self, widget, ctx, x, y, data, info, timestamp, row, col):
        widget.get_style_context().remove_class("pad-dragover")
        txt = data.get_text()
        if txt and "," in txt and not txt.startswith("/"):
            try: src_r, src_c = map(int, txt.strip().split(","))
            except ValueError: pass
            else:
                if (src_r, src_c) != (row, col):
                    mods = Gtk.get_current_event_state()[1] if Gtk.get_current_event_state()[0] else 0
                    copy = bool(mods & Gdk.ModifierType.CONTROL_MASK)
                    sp = self.cfg["pages"][self._page].get(str(self._note_for(src_r, src_c)))
                    if sp:
                        import copy as _copy
                        self._set_pad(row, col, _copy.deepcopy(sp))
                        if not copy: self._set_pad(src_r, src_c, None)
                Gtk.drag_finish(ctx, True, False, timestamp); return
        from urllib.parse import unquote, urlparse
        uris = data.get_uris()
        if not uris:
            Gtk.drag_finish(ctx, False, False, timestamp); return
        path = unquote(urlparse(uris[0]).path)
        if not Path(path).exists():
            Gtk.drag_finish(ctx, False, False, timestamp); return
        pad = dict(self._get_pad(row, col) or {})
        pad["file"] = path
        if not pad.get("label"): pad["label"] = Path(path).stem
        pad.setdefault("action","play")
        result = pad_edit_dialog(self._win, pad, self.audio.list_sinks(refresh=True),
                                 self.cfg.get("default_sink","vm_game"),
                                 self.audio, self._all_pad_keys())
        if result is not None: self._set_pad(row, col, result)
        Gtk.drag_finish(ctx, True, False, timestamp)

    def _open_settings(self, *_):
        result = settings_dialog(self._win, self.cfg, self.audio.list_sinks(refresh=True))
        if result is not None:
            self.cfg.update(result); save_config(self.cfg)
            self._mon_btn.handler_block_by_func(self._on_mon_toggled)
            self._mon_btn.set_active(self.cfg.get("monitor_enabled", False))
            self._mon_btn.handler_unblock_by_func(self._on_mon_toggled)
            self._update_mon_btn(); self._led_dirty = True; self._refresh_all()

    def _on_mon_toggled(self, btn):
        self.cfg["monitor_enabled"] = btn.get_active()
        save_config(self.cfg); self._update_mon_btn()

    def _update_mon_btn(self):
        if not self._mon_btn: return
        on  = self.cfg.get("monitor_enabled", False)
        ctx = self._mon_btn.get_style_context()
        self._mon_btn.set_label("Mon" + (" ●" if on else ""))
        # .stat-ok is a *label* class (colour + font-size only). Applied to a
        # ToggleButton it left the theme's default toggle background showing,
        # which renders bright pink on Adwaita dark. .act-on is a real button
        # style that matches the rest of the top bar.
        ctx.remove_class("stat-ok")
        if on: ctx.remove_class("act");    ctx.add_class("act-on")
        else:  ctx.remove_class("act-on"); ctx.add_class("act")
        sink = self.cfg.get("monitor_sink","?")
        self._mon_btn.set_tooltip_text(
            f"Monitor ON -> {sink}\nRight-click for settings" if on
            else "Monitor off\nRight-click for settings")

    def _update_rac(self, data):
        if self._rac_lbl:
            ctx = self._rac_lbl.get_style_context()
            ctx.remove_class("stat-ok"); ctx.remove_class("stat-dim")
            if self._rac_online:
                self._rac_lbl.set_text("● Hearth"); ctx.add_class("stat-ok")
                self._rac_vol_box.show_all()
                if getattr(self, "_hrth_btn", None) is not None: self._hrth_btn.show()
            else:
                self._rac_lbl.set_text("○ Hearth"); ctx.add_class("stat-dim")
                self._rac_vol_box.hide()
                if getattr(self, "_hrth_btn", None) is not None: self._hrth_btn.hide()
        if data:
            for sink, lbl in self._rac_vols.items():
                info = data.get(sink,{}); mute=info.get("mute",False); vol=info.get("vol","?")
                lbl.set_text(f"{'M ' if mute else ''}{vol}%")
        return False

    def _setup_tray(self):
        if not (HAVE_IND and AI):
            print("[tray] libappindicator3 unavailable -- closing the window "
                  "will quit instead of hiding")
            return
        icon = str(HOME/".local/share/icons/hicolor/48x48/apps/padfire.png")
        if not Path(icon).exists(): icon = "audio-x-generic"
        ind = AI.Indicator.new("padfire", icon, AI.IndicatorCategory.APPLICATION_STATUS)
        ind.set_status(AI.IndicatorStatus.ACTIVE); ind.set_title("Padfire")
        self._indicator = ind
        menu = Gtk.Menu()
        for lbl, fn in [("Show Padfire", self._show), ("Stop all", self.audio.stop_all),
                         ("Settings", self._open_settings), ("Quit", self._quit)]:
            it = Gtk.MenuItem(label=lbl); it.connect("activate", lambda _, f=fn: f()); menu.append(it)
        menu.show_all(); ind.set_menu(menu)
        self._update_tray()

    def _update_tray(self):
        # The README promised a "tray icon with page indicator"; v2.2 only ever
        # called set_title("Padfire") and never showed the page anywhere.
        ind = getattr(self, "_indicator", None)
        if ind is None: return False
        try:
            ind.set_label(f"P{self._page+1}", "P8")
            ind.set_title(f"Padfire -- page {self._page+1}")
        except Exception: pass
        return False

    def _on_close(self, *_):
        # With no tray icon, hiding made the window unrecoverable except via
        # `padfire-launch --show` from a terminal.
        if getattr(self, "_indicator", None) is not None:
            if self._win: self._win.hide()
            return True
        self._quit()
        return True

    def _on_key(self, _w, ev):
        # Esc hides to tray (or minimises when there is no tray), Ctrl+Q
        # quits, 1-8 switch pages like the Launchpad's top row, Space stops
        # everything. Pads are mouse/Launchpad-driven, so none of these
        # interfere with pad activation.
        ctrl = bool(ev.state & Gdk.ModifierType.CONTROL_MASK)
        if ev.keyval == Gdk.KEY_Escape:
            if getattr(self, "_indicator", None) is not None:
                if self._win: self._win.hide()
            elif self._win:
                self._win.iconify()
            return True
        if ctrl and ev.keyval in (Gdk.KEY_q, Gdk.KEY_Q):
            self._quit(); return True
        if not ctrl:
            for _i in range(NUM_PAGES):
                if ev.keyval in (getattr(Gdk, f"KEY_{_i+1}"),
                                 getattr(Gdk, f"KEY_KP_{_i+1}")):
                    self._switch_page(_i); return True
            if ev.keyval == Gdk.KEY_space:
                threading.Thread(target=self.audio.stop_all, daemon=True).start()
                return True
        return False

    def _show(self, *_):
        if self._win: self._win.present()
        return False

    def _quit(self, *_):
        if self._shutdown_event.is_set():
            return False
        self._shutdown_event.set()
        if self.midi:
            self.midi.shutdown()
        self.audio.stop_all()
        self._stop_script_processes()

        # Join script workers that may still be returning from Popen or
        # communicate(); generation invalidation above makes late Popen
        # completions terminate their child instead of registering it.
        with self._proc_lock:
            script_threads = list(self._script_threads)
        deadline = time.monotonic() + 1.5
        for thread in script_threads:
            if thread is threading.current_thread():
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            thread.join(timeout=remaining)

        # Retrigger and Stop All use a non-blocking reaper so their input
        # callbacks stay responsive. Quit owns those workers and waits for
        # their bounded process-group cleanup before leaving the main loop.
        with self._proc_lock:
            reaper_threads = list(self._script_reapers)
        for thread in reaper_threads:
            if thread is threading.current_thread():
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            thread.join(timeout=remaining)

        # The VU worker owns a parec subprocess and the Hearth probe owns a
        # long-lived polling loop. Both observe _shutdown_event and clean up.
        for thread in (self._vu_thread, self._rac_thread):
            if thread is not None and thread is not threading.current_thread():
                thread.join(timeout=1.5)

        for srv in self._ipc_ref:
            try:
                srv.close()
            except Exception:
                pass
        Path(SOCK).unlink(missing_ok=True)
        Path(PIDF).unlink(missing_ok=True)
        Gtk.main_quit()
        return False

    def run(self):
        signal.signal(signal.SIGTERM, lambda *_: GLib.idle_add(self._quit))
        signal.signal(signal.SIGINT,  lambda *_: GLib.idle_add(self._quit))
        CFGDIR.mkdir(parents=True, exist_ok=True)
        Path(PIDF).write_text(str(os.getpid()))
        Gtk.main()


def install_service():
    svc = Path.home()/".config/systemd/user/padfire.service"
    # prefer the launcher shipped next to this file, then fall back to ~/bin
    _here = Path(__file__).resolve().parent/"padfire-launch"
    launcher = _here if _here.exists() else Path.home()/"bin/padfire-launch"
    if not launcher.exists():
        print(f"error: no padfire-launch found "
              f"(looked in {_here.parent} and {Path.home()/'bin'})")
        return 1
    svc.parent.mkdir(parents=True, exist_ok=True)
    # No hardcoded WAYLAND_DISPLAY: it broke X11 sessions and any Wayland
    # socket not named wayland-0. graphical-session.target already carries the
    # session environment. This now matches packaging/padfire.service.
    svc.write_text(f"""[Unit]
Description=Padfire Soundboard
PartOf=graphical-session.target
After=graphical-session.target pipewire.service wireplumber.service

[Service]
Type=simple
ExecStart={launcher}
Restart=on-failure
RestartSec=4

[Install]
WantedBy=graphical-session.target
""")
    print(f"wrote {svc}")
    rc = 0
    # v2.2 used os.system and ignored both exit codes, so a failed enable
    # still printed nothing and looked like success
    for cmd in (["systemctl", "--user", "daemon-reload"],
                ["systemctl", "--user", "enable", "padfire.service"]):
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            rc = r.returncode
            print(f"error: {' '.join(cmd)} failed: {r.stderr.strip()}")
    if rc == 0:
        print("enabled. start it now with: systemctl --user start padfire")
    return rc


def main():
    GLib.set_prgname("padfire")
    ap = argparse.ArgumentParser(description=f"Padfire {VER}")
    ap.add_argument("--version", action="version", version=f"Padfire {VER}")
    ap.add_argument("--quit",    action="store_true",
                    help="quit the running instance")
    ap.add_argument("--show",    action="store_true",
                    help="bring the window to the front")
    ap.add_argument("--reload",  action="store_true",
                    help="re-read padfire.json without restarting")
    ap.add_argument("--stop",    action="store_true",
                    help="stop all audio")
    ap.add_argument("--status",  action="store_true",
                    help="print JSON status (for Hearth and scripts)")
    ap.add_argument("--install", action="store_true",
                    help="write and enable the systemd --user service")
    args = ap.parse_args()
    if args.install: raise SystemExit(install_service() or 0)
    # Order preserved from the original: action flags win over --status.
    for _flag, _cmd in (("quit", "quit"), ("show", "show"),
                        ("reload", "reload"), ("stop", "stop_all")):
        if getattr(args, _flag):
            _resp = ipc_send(_cmd)
            print(_resp or "not running")
            # Exit non-zero when nothing was listening, so a script or a
            # systemd unit can tell "done" from "no instance answered".
            sys.exit(0 if _resp else 1)
    if args.status:
        # --status is a machine contract ("JSON status for Hearth/tools"),
        # so it must stay parseable with no instance running. The old code
        # printed the bare text "not running" and still exited 0, which
        # made every JSON consumer raise on the first character.
        _resp = ipc_send("status")
        print(_resp if _resp else json.dumps({"running": False,
                                              "version": VER}))
        return
    if ipc_send("status"):
        print("[padfire] already running -- use --show"); return
    PadfireApp().run()

if __name__ == "__main__":
    main()
