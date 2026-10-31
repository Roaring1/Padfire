"""Windows-capable audio playback using PortAudio/WASAPI.

The callback-based engine keeps pad logic independent of PulseAudio/PipeWire,
supports polyphony and looping, and lets each assignment choose an output
device. Monitor playback is a second stream to the selected monitor device.
"""
from __future__ import annotations

import math
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import numpy as np
import sounddevice as sd
import soundfile as sf


@dataclass
class _Playback:
    stop: threading.Event
    thread: threading.Thread | None = None
    stream: Any = None


class AudioEngine:
    def __init__(self, *, disabled: bool = False) -> None:
        self.disabled = disabled
        self._lock = threading.RLock()
        self._streams: dict[str, _Playback] = {}
        self._sinks: list[str] = []
        self._level = 0.0
        self._refresh_sinks()

    def _refresh_sinks(self) -> None:
        self._sinks = []
        if self.disabled:
            return
        try:
            devices = sd.query_devices()
            host_apis = sd.query_hostapis()
            for device in devices:
                if int(device.get("max_output_channels", 0)) > 0:
                    host_index = int(device.get("hostapi", -1))
                    host_name = (
                        str(host_apis[host_index].get("name", "Audio"))
                        if 0 <= host_index < len(host_apis) else "Audio"
                    )
                    # PortAudio indices are not stable between boots or when
                    # USB endpoints are added. Persist the host API + endpoint
                    # label, then resolve it against the current enumeration.
                    self._sinks.append(f"{host_name}: {device['name']}")
        except Exception as exc:
            print(f"[audio] could not enumerate output devices: {exc}")

    def list_sinks(self, refresh: bool = False) -> list[str]:
        if refresh or not self._sinks:
            self._refresh_sinks()
        return list(self._sinks)

    def _resolve_device(self, name: str | None) -> int | None:
        if self.disabled:
            return None
        if name:
            wanted = name.strip().casefold()
            try:
                devices = sd.query_devices()
                host_apis = sd.query_hostapis()
                for index, device in enumerate(devices):
                    if int(device.get("max_output_channels", 0)) <= 0:
                        continue
                    host_index = int(device.get("hostapi", -1))
                    host_name = (
                        str(host_apis[host_index].get("name", "Audio"))
                        if 0 <= host_index < len(host_apis) else "Audio"
                    )
                    stable_name = f"{host_name}: {device['name']}".casefold()
                    # Accept the stable token and the plain endpoint name for
                    # older/pre-release Windows configs.
                    if stable_name == wanted or str(device["name"]).casefold() == wanted:
                        return index
            except Exception as exc:
                print(f"[audio] device resolution failed: {exc}")
            # Linux sink names in a shared config are not Windows device IDs.
            # An unknown device therefore falls back safely to the OS default.
        try:
            default = sd.default.device
            output = default[1] if isinstance(default, (tuple, list)) else default
            return int(output) if output is not None and int(output) >= 0 else None
        except Exception:
            return None

    def play(
        self,
        key: str,
        filepath: str,
        sink_name: str | None,
        vol: int,
        loop: bool,
        monitor_sink: str | None = None,
    ) -> None:
        if self.disabled:
            return
        self.stop(key)
        try:
            samples, sample_rate = sf.read(
                filepath, dtype="float32", always_2d=True
            )
            if samples.size == 0:
                raise ValueError("audio file contains no samples")
            if samples.shape[1] > 8:
                raise ValueError("audio files with more than eight channels are unsupported")
        except Exception as exc:
            print(f"[audio] {key}: cannot read {filepath!r}: {exc}")
            return

        gain = max(0.0, min(150, int(vol))) / 100.0
        output = self._resolve_device(sink_name)
        self._start_stream(key, samples, int(sample_rate), output, gain, loop)
        if monitor_sink and monitor_sink != sink_name:
            monitor_output = self._resolve_device(monitor_sink)
            if monitor_output != output:
                self._start_stream(
                    f"__mon__{key}", samples, int(sample_rate),
                    monitor_output, gain, loop, update_meter=False
                )

    def _start_stream(
        self,
        key: str,
        samples: np.ndarray,
        sample_rate: int,
        device: int | None,
        gain: float,
        loop: bool,
        *,
        update_meter: bool = True,
    ) -> None:
        playback = _Playback(stop=threading.Event())
        channels = int(samples.shape[1])
        position = 0

        def callback(outdata: np.ndarray, frames: int, _time: Any, status: Any) -> None:
            nonlocal position
            if playback.stop.is_set():
                outdata.fill(0)
                raise sd.CallbackStop
            if status:
                print(f"[audio] stream warning ({key}): {status}")
            rendered = np.zeros((frames, channels), dtype=np.float32)
            offset = 0
            sample_count = len(samples)
            while offset < frames:
                available = sample_count - position
                if available <= 0:
                    if loop:
                        position = 0
                        available = sample_count
                    else:
                        break
                take = min(frames - offset, available)
                rendered[offset:offset + take] = samples[position:position + take]
                offset += take
                position += take
            rendered *= gain
            outdata[:] = rendered
            if update_meter and rendered.size:
                rms = float(np.sqrt(np.mean(np.square(rendered))))
                with self._lock:
                    self._level = max(self._level * 0.72, min(1.0, rms * 2.0))
            if position >= sample_count and not loop:
                raise sd.CallbackStop

        def run() -> None:
            try:
                with sd.OutputStream(
                    device=device,
                    samplerate=sample_rate,
                    channels=channels,
                    dtype="float32",
                    callback=callback,
                ) as stream:
                    with self._lock:
                        if playback.stop.is_set():
                            stream.abort()
                        playback.stream = stream
                    while not playback.stop.wait(0.05):
                        if not stream.active:
                            break
            except Exception as exc:
                print(f"[audio] output failed ({key}): {exc}")
            finally:
                with self._lock:
                    if self._streams.get(key) is playback:
                        self._streams.pop(key, None)

        playback.thread = threading.Thread(
            target=run, name=f"padfire-audio-{key}", daemon=True
        )
        with self._lock:
            self._streams[key] = playback
        playback.thread.start()

    def stop(self, key: str) -> None:
        for stream_key in (key, f"__mon__{key}"):
            with self._lock:
                playback = self._streams.pop(stream_key, None)
            if playback is None:
                continue
            playback.stop.set()
            if playback.stream is not None:
                try:
                    playback.stream.abort()
                except Exception:
                    pass
            if playback.thread is not None and playback.thread is not threading.current_thread():
                playback.thread.join(timeout=0.35)

    def stop_all(self) -> None:
        with self._lock:
            keys = list(self._streams)
        for key in keys:
            if key.startswith("__mon__"):
                self.stop(key[len("__mon__"):])
            else:
                self.stop(key)

    def is_playing(self, key: str) -> bool:
        with self._lock:
            playback = self._streams.get(key)
            return playback is not None and not playback.stop.is_set()

    def active_keys(self) -> list[str]:
        with self._lock:
            return [
                key for key, playback in self._streams.items()
                if not key.startswith("__mon__") and not playback.stop.is_set()
            ]

    def level(self) -> float:
        with self._lock:
            value = self._level
            self._level *= 0.82
            return value

    @staticmethod
    def analyze_loudness(filepath: str) -> dict[str, float | bool] | None:
        """Analyze at most 120 seconds; pyloudnorm is an optional build dependency."""
        try:
            import pyloudnorm as pyln

            info = sf.info(filepath)
            frames = min(int(info.frames), int(info.samplerate * 120))
            data, rate = sf.read(
                filepath, frames=frames, dtype="float32", always_2d=True
            )
            if data.size == 0:
                return None
            if not np.any(data):
                return {"silent": True}
            meter_data = data[:, 0] if data.shape[1] == 1 else data[:, :2]
            loudness = float(pyln.Meter(rate).integrated_loudness(meter_data))
            peak = float(20.0 * math.log10(max(float(np.max(np.abs(data))), 1e-12)))
            if not math.isfinite(loudness) or not math.isfinite(peak):
                return {"silent": True}
            gain = 10.0 ** ((-18.0 - loudness) / 20.0)
            suggested = max(1, min(150, int(round(100.0 * gain))))
            return {
                "input_i": loudness,
                "input_tp": peak,
                "suggested_vol": float(suggested),
            }
        except Exception as exc:
            print(f"[lufs] analysis failed: {exc}")
            return None
