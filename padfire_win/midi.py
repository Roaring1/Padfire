"""Launchpad Mini MK1/MK2 MIDI discovery and protocol handling."""
from __future__ import annotations

import threading
import time
from typing import Callable, Any

import mido

from padfire_core import (
    CC_TOP, DEVICE_EXCLUDE, DEVICE_SUBSTR, LED_OFF,
    NUM_COLS, NUM_ROWS,
)


def note_xy(row: int, col: int) -> int:
    return row * 16 + col


def note_side(row: int) -> int:
    return row * 16 + 8


class MidiEngine:
    def __init__(
        self,
        on_pad: Callable[[int, int, bool], None],
        on_top: Callable[[int, bool], None],
        on_side: Callable[[int, bool], None],
        on_connect: Callable[[], None],
        on_disconnect: Callable[[], None],
    ) -> None:
        self._on_pad = on_pad
        self._on_top = on_top
        self._on_side = on_side
        self._on_connect = on_connect
        self._on_disconnect = on_disconnect
        self._in: Any = None
        self._out: Any = None
        self._lock = threading.RLock()
        self._out_lock = threading.Lock()
        self._stop = threading.Event()
        self._connected = False
        self._reader = threading.Thread(
            target=self._read_loop, name="padfire-midi-read", daemon=True
        )
        self._watcher = threading.Thread(
            target=self._reconnect_loop, name="padfire-midi-watch", daemon=True
        )
        self._reader.start()
        self._watcher.start()

    @staticmethod
    def _find(names: list[str]) -> str | None:
        for name in names:
            if DEVICE_SUBSTR not in name:
                continue
            if any(part in name for part in DEVICE_EXCLUDE):
                continue
            return name
        return None

    def try_connect(self) -> bool:
        with self._lock:
            if self._connected:
                return True
            input_name = self._find(mido.get_input_names())
            output_name = self._find(mido.get_output_names())
            if not input_name or not output_name:
                return False
            input_port = output_port = None
            try:
                output_port = mido.open_output(output_name)
                input_port = mido.open_input(input_name)
                self._out = output_port
                self._in = input_port
                self._connected = True
                print(f"[midi] connected: {input_name}")
            except Exception as exc:
                print(f"[midi] connection failed: {exc}")
                for port in (input_port, output_port):
                    if port is not None:
                        try:
                            port.close()
                        except Exception:
                            pass
                return False
        self._on_connect()
        return True

    def _disconnect(self) -> None:
        with self._lock:
            was_connected = self._connected
            self._connected = False
            ports = (self._in, self._out)
            self._in = self._out = None
        for port in ports:
            if port is not None:
                try:
                    port.close()
                except Exception:
                    pass
        if was_connected:
            self._on_disconnect()

    def _reconnect_loop(self) -> None:
        while not self._stop.wait(2.0):
            if not self.is_connected():
                try:
                    self.try_connect()
                except Exception as exc:
                    print(f"[midi] discovery failed: {exc}")

    def _read_loop(self) -> None:
        while not self._stop.is_set():
            with self._lock:
                port = self._in if self._connected else None
            if port is None:
                self._stop.wait(0.1)
                continue
            try:
                for message in port:
                    if self._stop.is_set():
                        break
                    self._dispatch(message)
                if not self._stop.is_set() and self.is_connected():
                    self._disconnect()
            except Exception as exc:
                if not self._stop.is_set():
                    print(f"[midi] input disconnected: {exc}")
                    self._disconnect()

    def _dispatch(self, message: Any) -> None:
        if message.type in ("note_on", "note_off"):
            row, col = divmod(message.note, 16)
            if not 0 <= row < NUM_ROWS:
                return
            pressed = message.type == "note_on" and message.velocity > 0
            if col == 8:
                self._on_side(row, pressed)
            elif 0 <= col < NUM_COLS:
                self._on_pad(row, col, pressed)
        elif message.type == "control_change" and message.control in CC_TOP:
            self._on_top(message.control - 104, message.value > 0)

    def _send(self, message: Any) -> None:
        with self._out_lock:
            with self._lock:
                port = self._out
            if port is not None:
                try:
                    port.send(message)
                except Exception as exc:
                    print(f"[midi] LED send failed: {exc}")

    def led(self, note: int, color: int) -> None:
        self._send(mido.Message("note_on", note=note, velocity=color, channel=0))

    def cc_led(self, control: int, color: int) -> None:
        self._send(mido.Message("control_change", control=control, value=color, channel=0))

    def clear_all(self) -> None:
        for row in range(NUM_ROWS):
            for col in range(NUM_COLS):
                self.led(note_xy(row, col), LED_OFF)
            self.led(note_side(row), LED_OFF)
        for control in CC_TOP:
            self.cc_led(control, LED_OFF)

    def is_connected(self) -> bool:
        with self._lock:
            return self._connected

    def shutdown(self) -> None:
        self._stop.set()
        if self.is_connected():
            try:
                self.clear_all()
            except Exception:
                pass
        self._disconnect()
        for thread in (self._reader, self._watcher):
            if thread is not threading.current_thread():
                thread.join(timeout=0.5)
