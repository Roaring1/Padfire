#!/usr/bin/env python3
"""
padfire_test.py  —  Phase 1 MIDI smoke test
Confirmed device: Launchpad Mini (MK1/MK2), USB 1235:0036
Layout: X-Y mode  note = row*16 + col  (row 0 = top, col 0 = left)
Top row: CC 104-111  |  Side buttons: row*16 + 8
LED velocity: (green & 3) << 4 | (red & 3)  — bi-color only (no blue)
"""

import mido
import sys
import signal
import time

# Force line-buffered stdout so output reaches the file in real time
sys.stdout.reconfigure(line_buffering=True)

# ── LED color constants (velocity byte) ───────────────────────────────────
COLOR_OFF    = 0x00
COLOR_RED    = 0x03
COLOR_ORANGE = 0x13
COLOR_AMBER  = 0x33
COLOR_GREEN  = 0x30
COLOR_DIM    = 0x10

# ── Hardware layout ────────────────────────────────────────────────────────
GRID_NOTES = [row * 16 + col for row in range(8) for col in range(8)]
SIDE_NOTES = [row * 16 + 8   for row in range(8)]
TOP_CCS    = list(range(104, 112))

DEVICE_SUBSTR = "Launchpad Mini"

def note_to_rowcol(note):
    return note // 16, note % 16

# ── LED helpers ────────────────────────────────────────────────────────────
def led(out, note, color):
    out.send(mido.Message('note_on', note=note, velocity=color, channel=0))

def cc_led(out, cc, color):
    out.send(mido.Message('control_change', control=cc, value=color, channel=0))

def clear_all(out):
    for n in GRID_NOTES + SIDE_NOTES:
        led(out, n, COLOR_OFF)
    for c in TOP_CCS:
        cc_led(out, c, COLOR_OFF)
    time.sleep(0.25)

def light_welcome(out):
    for n in GRID_NOTES:
        led(out, n, COLOR_GREEN)
    for n in SIDE_NOTES:
        led(out, n, COLOR_RED)
    for c in TOP_CCS:
        cc_led(out, c, COLOR_AMBER)

# ── Port detection ─────────────────────────────────────────────────────────
def find_port(names):
    for n in names:
        if DEVICE_SUBSTR in n:
            return n
    return None

# ── Main ───────────────────────────────────────────────────────────────────
def main():
    in_name  = find_port(mido.get_input_names())
    out_name = find_port(mido.get_output_names())

    if not in_name or not out_name:
        print("ERROR: Launchpad Mini not found. Ports seen:")
        for p in mido.get_input_names():
            print(f"  {p}")
        sys.exit(1)

    print(f"[padfire_test] in  = {in_name}")
    print(f"[padfire_test] out = {out_name}")

    led_state: dict[int, int] = {}

    with mido.open_output(out_name) as out, \
         mido.open_input(in_name)  as inp:

        def shutdown(sig, frame):
            print("\n[padfire_test] clearing LEDs, bye.")
            clear_all(out)
            sys.exit(0)

        signal.signal(signal.SIGINT,  shutdown)
        signal.signal(signal.SIGTERM, shutdown)

        print("[padfire_test] lighting pads — press pads, Ctrl+C to exit\n")
        light_welcome(out)

        for msg in inp:
            if msg.type == 'note_on' and msg.velocity > 0:
                n = msg.note
                row, col = note_to_rowcol(n)
                is_side  = col == 8
                kind     = "SIDE" if is_side else "GRID"
                print(f"[{kind}] note={n:3d}  row={row} col={col}")
                cur = led_state.get(n, COLOR_GREEN)
                nxt = COLOR_RED if cur != COLOR_RED else COLOR_GREEN
                led_state[n] = nxt
                led(out, n, nxt)

            elif msg.type == 'note_off' or \
                 (msg.type == 'note_on' and msg.velocity == 0):
                row, col = note_to_rowcol(msg.note)
                print(f"[REL ] note={msg.note:3d}  row={row} col={col}")

            elif msg.type == 'control_change':
                idx = msg.control - 104
                print(f"[TOP ] cc={msg.control}  idx={idx}  value={msg.value}")
                if msg.value > 0:
                    cc_led(out, msg.control, COLOR_RED)
                else:
                    cc_led(out, msg.control, COLOR_AMBER)

            else:
                print(f"[????] {msg}")

if __name__ == '__main__':
    main()
