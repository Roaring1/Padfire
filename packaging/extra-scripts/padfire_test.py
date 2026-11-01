#!/usr/bin/env python3
"""Manual Launchpad Mini MK1/MK2 input and LED smoke test.

SAFETY:
- Quit Padfire before running this test; it deliberately opens the MIDI ports.
- It lights the controller during the test and clears the LEDs on exit.
- The MK3 is explicitly rejected because it uses a different protocol.
- This script is a manual hardware diagnostic, not part of automated CI.
"""
from __future__ import annotations

import json
import os
import signal
import socket
import stat
import sys
import time
from pathlib import Path

import mido


GRID_NOTES = [row * 16 + col for row in range(8) for col in range(8)]
SIDE_NOTES = [row * 16 + 8 for row in range(8)]
TOP_CCS = list(range(104, 112))

# Keep these identical to Padfire's known-good Launchpad Mini MK1/MK2 protocol.
LED_OFF = 0x0C
LED_DIM = 0x1C
LED_GREEN = 0x3C
LED_RED = 0x0F
LED_ORANGE = 0x1F
LED_AMBER = 0x3F

DEVICE_SUBSTR = "launchpad mini"
DEVICE_EXCLUDE = ("mk3",)


def is_supported_port(name: str) -> bool:
    lowered = name.casefold()
    return DEVICE_SUBSTR in lowered and not any(
        marker in lowered for marker in DEVICE_EXCLUDE
    )


def padfire_is_running(socket_path: Path | None = None) -> bool | None:
    """Return True/False for a live socket, or None for an unsafe socket path."""
    path = socket_path or (Path.home() / ".config" / "padfire" / "padfire.sock")
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    except OSError as exc:
        print(f"ERROR: Cannot inspect Padfire IPC path: {exc}", file=sys.stderr)
        return None

    if not stat.S_ISSOCK(info.st_mode):
        print(f"ERROR: Refusing non-socket Padfire IPC path: {path}", file=sys.stderr)
        return None
    if hasattr(os, "getuid") and info.st_uid != os.getuid():
        print(f"ERROR: Padfire IPC socket is owned by another user: {path}", file=sys.stderr)
        return None

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
            conn.settimeout(0.5)
            conn.connect(str(path))
            conn.sendall(b"status\n")
            data = bytearray()
            while b"\n" not in data and len(data) <= 65536:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                data.extend(chunk)
        if not data or len(data) > 65536:
            return None
        status = json.loads(bytes(data).split(b"\n", 1)[0].decode("utf-8"))
        return bool(status.get("running", True))
    except (ConnectionRefusedError, FileNotFoundError):
        return False
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        print(f"ERROR: Could not safely query Padfire IPC: {exc}", file=sys.stderr)
        return None


def note_to_rowcol(note: int) -> tuple[int, int]:
    return note // 16, note % 16


def led(out: mido.ports.BaseOutput, note: int, color: int) -> None:
    out.send(mido.Message("note_on", note=note, velocity=color, channel=0))


def cc_led(out: mido.ports.BaseOutput, cc: int, color: int) -> None:
    out.send(mido.Message("control_change", control=cc, value=color, channel=0))


def clear_all(out: mido.ports.BaseOutput) -> None:
    for note in GRID_NOTES + SIDE_NOTES:
        led(out, note, LED_OFF)
    for cc in TOP_CCS:
        cc_led(out, cc, LED_OFF)
    time.sleep(0.25)


def light_welcome(out: mido.ports.BaseOutput) -> None:
    for note in GRID_NOTES:
        led(out, note, LED_GREEN)
    for note in SIDE_NOTES:
        led(out, note, LED_ORANGE)
    for cc in TOP_CCS:
        cc_led(out, cc, LED_AMBER)


def find_port(names: list[str]) -> str | None:
    return next((name for name in names if is_supported_port(name)), None)


def main() -> int:
    active = padfire_is_running()
    if active is None:
        print("ERROR: Cannot prove Padfire is stopped; refusing to claim the MIDI ports.")
        return 2
    if active:
        print("ERROR: Padfire is currently running. Quit it before this hardware smoke test.")
        return 2

    try:
        input_names = mido.get_input_names()
        output_names = mido.get_output_names()
    except Exception as exc:
        print(f"ERROR: MIDI backend unavailable: {exc}", file=sys.stderr)
        return 1

    in_name = find_port(input_names)
    out_name = find_port(output_names)
    if not in_name or not out_name:
        print("ERROR: Supported Launchpad Mini MK1/MK2 not found. MIDI ports seen:")
        for name in sorted(set(input_names + output_names)):
            print(f"  {name}")
        print("The Launchpad Mini MK3 is intentionally excluded.")
        return 1

    print(f"[padfire_test] input  = {in_name}")
    print(f"[padfire_test] output = {out_name}")
    print("[padfire_test] This test will light the grid. Press pads; Ctrl+C to exit.")

    led_state: dict[int, int] = {}
    try:
        with mido.open_output(out_name) as out:
            with mido.open_input(in_name) as inp:
                def request_stop(_signum, _frame):
                    raise KeyboardInterrupt

                try:
                    signal.signal(signal.SIGINT, request_stop)
                    signal.signal(signal.SIGTERM, request_stop)
                    light_welcome(out)
                    for msg in inp:
                        if msg.type == "note_on" and msg.velocity > 0:
                            row, col = note_to_rowcol(msg.note)
                            if not (0 <= row < 8 and 0 <= col <= 8):
                                print(f"[OTHER] {msg}")
                                continue
                            kind = "SIDE" if col == 8 else "GRID"
                            print(f"[{kind}] note={msg.note:3d} row={row} col={col}")
                            current = led_state.get(msg.note, LED_GREEN)
                            next_color = LED_RED if current != LED_RED else LED_GREEN
                            led_state[msg.note] = next_color
                            led(out, msg.note, next_color)
                        elif msg.type == "note_off" or (
                            msg.type == "note_on" and msg.velocity == 0
                        ):
                            row, col = note_to_rowcol(msg.note)
                            print(f"[REL ] note={msg.note:3d} row={row} col={col}")
                        elif msg.type == "control_change" and msg.control in TOP_CCS:
                            index = msg.control - 104
                            print(f"[TOP ] cc={msg.control} page={index + 1} value={msg.value}")
                            cc_led(out, msg.control, LED_RED if msg.value else LED_AMBER)
                        else:
                            print(f"[OTHER] {msg}")
                finally:
                    print("[padfire_test] clearing LEDs.")
                    clear_all(out)
    except KeyboardInterrupt:
        print("[padfire_test] stopped.")
    except Exception as exc:
        print(f"ERROR: MIDI smoke test failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
