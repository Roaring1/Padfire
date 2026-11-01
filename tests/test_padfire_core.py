#!/usr/bin/env python3
"""Fast Padfire v2.3 regression checks; never opens MIDI/audio or real user config."""
import importlib.util
import json
import os
import shutil
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
TEST_HOME = Path(tempfile.mkdtemp(prefix="padfire-core-test-home-"))
os.environ["HOME"] = str(TEST_HOME)

# Import only after HOME points to a disposable directory: padfire.py performs
# a one-time legacy-config migration check at import time.
spec = importlib.util.spec_from_file_location("padfire_under_test", ROOT / "padfire.py")
assert spec and spec.loader
padfire = importlib.util.module_from_spec(spec)
spec.loader.exec_module(padfire)


class PadfireCoreTests(unittest.TestCase):
    def setUp(self):
        self.config_dir = TEST_HOME / ".config" / "padfire-tests" / self._testMethodName
        self.config_dir.mkdir(parents=True, exist_ok=True)
        padfire.CFGDIR = self.config_dir
        padfire.CONF = str(self.config_dir / "padfire.json")

    def test_migration_normalizes_legacy_config_and_preserves_toggle(self):
        old = {
            "version": 3,
            "device": "dead legacy setting",
            "retrigger": "toggle",
            "pages": [{"0": {"file": "/tmp/example.wav", "mode": "old"}}],
        }
        migrated = padfire._migrate(old)
        self.assertEqual(migrated["version"], padfire.CONFIG_VERSION)
        self.assertEqual(len(migrated["pages"]), 8)
        self.assertEqual(migrated["retrigger"], "toggle")
        self.assertNotIn("device", migrated)
        self.assertNotIn("mode", migrated["pages"][0]["0"])
        self.assertEqual(migrated["pages"][0]["0"]["action"], "play")

    def test_invalid_retrigger_defaults_to_restart(self):
        migrated = padfire._migrate({"version": 5, "retrigger": "unknown", "pages": []})
        self.assertEqual(migrated["retrigger"], "restart")
        self.assertEqual(len(migrated["pages"]), 8)

    def test_migration_preserves_crosshair_toggle_pad_action(self):
        cfg = {
            "version": 5,
            "pages": [{"17": {"action": "crosshair_toggle", "label": "Aim"}}],
        }
        migrated = padfire._migrate(cfg)
        self.assertEqual(
            migrated["pages"][0]["17"],
            {"action": "crosshair_toggle", "label": "Aim"},
        )

    def test_crosshair_toggle_command_prefers_path(self):
        with patch.object(padfire.shutil, "which", return_value="/usr/bin/crosshair"):
            self.assertEqual(
                padfire.crosshair_toggle_argv(),
                ["/usr/bin/crosshair", "--toggle"],
            )

    def test_crosshair_toggle_command_uses_local_bin_fallback(self):
        fallback = TEST_HOME / ".local" / "bin" / "crosshair"
        fallback.parent.mkdir(parents=True, exist_ok=True)
        fallback.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        fallback.chmod(0o755)
        with patch.object(padfire.shutil, "which", return_value=None):
            self.assertEqual(
                padfire.crosshair_toggle_argv(),
                [str(fallback), "--toggle"],
            )

    def test_crosshair_toggle_command_reports_missing_cli(self):
        with patch.object(padfire.shutil, "which", return_value=None):
            self.assertIsNone(padfire.crosshair_toggle_argv())

    def test_save_keeps_previous_generation_as_backup(self):
        first = {"version": 5, "label": "prior"}
        second = {"version": 5, "label": "current"}
        Path(padfire.CONF).write_text(json.dumps(first), encoding="utf-8")
        padfire.save_config(second)
        self.assertEqual(json.loads(Path(padfire.CONF).read_text()), second)
        self.assertEqual(json.loads(Path(padfire.CONF + ".bak").read_text()), first)
        self.assertFalse(Path(padfire.CONF + ".tmp").exists())

    def test_corrupt_config_falls_back_to_safe_defaults(self):
        Path(padfire.CONF).write_text("{ broken json", encoding="utf-8")
        fallback = padfire.load_config()
        self.assertEqual(fallback["version"], padfire.CONFIG_VERSION)
        self.assertEqual(len(fallback["pages"]), 8)
        self.assertEqual(fallback["retrigger"], "restart")

    def test_midi_discovery_rejects_mk3_but_accepts_supported_mini(self):
        engine = object.__new__(padfire.MidiEngine)  # _find itself is pure and starts no threads
        found = engine._find(["Launchpad Mini MK3 MIDI 1", "Launchpad Mini MIDI 1"])
        self.assertEqual(found, "Launchpad Mini MIDI 1")
        self.assertIsNone(engine._find(["Launchpad Mini MK3 MIDI 1", "Other controller"]))

    def test_audio_stop_and_stop_all_invalidate_workers_not_started_yet(self):
        class DeferredThread:
            def __init__(self, target, args=(), daemon=None, name=None):
                self.target = target
                self.args = args

            def start(self):
                workers.append(self)

            def run(self):
                self.target(*self.args)

        for stop_all in (False, True):
            with self.subTest(stop_all=stop_all):
                engine = object.__new__(padfire.AudioEngine)
                engine._procs = {}
                engine._generations = {}
                engine._lock = threading.Lock()
                engine._sinks = ["vm_game"]
                workers = []
                with patch.object(padfire.threading, "Thread", DeferredThread):
                    with patch.object(padfire.subprocess, "Popen") as popen:
                        engine.play("0:1", "/tmp/sound.wav", "vm_game", 80, False)
                        if stop_all:
                            engine.stop_all()
                        else:
                            engine.stop("0:1")
                        self.assertEqual(len(workers), 1)
                        workers[0].run()
                        popen.assert_not_called()

    def test_newer_playback_invalidates_older_pending_worker(self):
        class DeferredThread:
            def __init__(self, target, args=(), daemon=None, name=None):
                self.target = target
                self.args = args

            def start(self):
                workers.append(self)

            def run(self):
                self.target(*self.args)

        class FinishedProcess:
            returncode = 0

            def communicate(self):
                return b"", b""

            def poll(self):
                return self.returncode

            def wait(self, timeout=None):
                return self.returncode

            def kill(self):
                self.returncode = -9

        engine = object.__new__(padfire.AudioEngine)
        engine._procs = {}
        engine._generations = {}
        engine._lock = threading.Lock()
        engine._sinks = ["vm_game"]
        workers = []
        with patch.object(padfire.threading, "Thread", DeferredThread):
            engine.play("0:2", "/tmp/old.wav", "vm_game", 80, False)
            engine.play("0:2", "/tmp/new.wav", "vm_game", 80, False)
        self.assertEqual(len(workers), 2)

        with patch.object(padfire.subprocess, "Popen", return_value=FinishedProcess()) as popen:
            workers[0].run()
            popen.assert_not_called()
            workers[1].run()
            self.assertEqual(popen.call_count, 1)
            self.assertIn("/tmp/new.wav", popen.call_args.args[0])

    def test_audio_stop_during_process_creation_kills_late_process(self):
        class PendingProcess:
            def __init__(self):
                self.returncode = None
                self.killed = threading.Event()

            def communicate(self):
                return b"", b""

            def poll(self):
                return self.returncode

            def kill(self):
                self.returncode = -9
                self.killed.set()

            def wait(self, timeout=None):
                return self.returncode

        engine = object.__new__(padfire.AudioEngine)
        engine._procs = {}
        engine._generations = {}
        engine._lock = threading.Lock()
        engine._sinks = ["vm_game"]
        entered_popen = threading.Event()
        release_popen = threading.Event()
        process = PendingProcess()
        workers = []
        real_thread = threading.Thread

        def tracked_thread(*args, **kwargs):
            worker = real_thread(*args, **kwargs)
            workers.append(worker)
            return worker

        def delayed_popen(*args, **kwargs):
            entered_popen.set()
            release_popen.wait(2)
            return process

        with patch.object(padfire.threading, "Thread", side_effect=tracked_thread):
            with patch.object(padfire.subprocess, "Popen", side_effect=delayed_popen):
                engine.play("0:3", "/tmp/racing.wav", "vm_game", 80, False)
                self.assertTrue(entered_popen.wait(1), "playback worker never reached Popen")
                engine.stop("0:3")
                release_popen.set()
                self.assertTrue(process.killed.wait(1), "late subprocess survived Stop")
                workers[0].join(timeout=1)
        self.assertFalse(engine.is_playing("0:3"))

    def _bare_midi_engine(self):
        engine = object.__new__(padfire.MidiEngine)
        engine._in = engine._out = None
        engine._out_lock = threading.Lock()
        engine._conn_lock = threading.Lock()
        engine._connected = False
        engine._running = True
        engine._on_connect = lambda: None
        engine._on_disconnect = lambda: None
        return engine

    def test_midi_input_open_failure_closes_already_open_output(self):
        class FakePort:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        output = FakePort()
        with patch.object(padfire.mido, "get_input_names", return_value=["Launchpad Mini MIDI 1"]), \
             patch.object(padfire.mido, "get_output_names", return_value=["Launchpad Mini MIDI 1"]), \
             patch.object(padfire.mido, "open_output", return_value=output), \
             patch.object(padfire.mido, "open_input", side_effect=OSError("device vanished")):
            self.assertFalse(engine.try_connect())

        self.assertTrue(output.closed)
        self.assertIsNone(engine._in)
        self.assertIsNone(engine._out)
        self.assertFalse(engine.is_connected())

    def test_midi_discovery_failure_does_not_prevent_a_later_retry(self):
        class FakePort:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        output = FakePort()
        input_port = FakePort()
        with patch.object(
            padfire.mido,
            "get_input_names",
            side_effect=[OSError("backend restarting"), ["Launchpad Mini MIDI 1"]],
        ), patch.object(
            padfire.mido, "get_output_names", return_value=["Launchpad Mini MIDI 1"]
        ), patch.object(
            padfire.mido, "open_output", return_value=output
        ), patch.object(
            padfire.mido, "open_input", return_value=input_port
        ):
            self.assertFalse(engine.try_connect())
            self.assertTrue(engine.try_connect())

        self.assertTrue(engine.is_connected())
        engine._disconnect()
        self.assertTrue(output.closed)
        self.assertTrue(input_port.closed)

    def test_midi_disconnect_closes_partial_handle_even_when_not_connected(self):
        class FakePort:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        output = FakePort()
        engine._out = output
        disconnect_events = []
        engine._on_disconnect = lambda: disconnect_events.append(True)

        engine._disconnect()

        self.assertTrue(output.closed)
        self.assertIsNone(engine._out)
        self.assertIsNone(engine._in)
        self.assertEqual(disconnect_events, [])

    def test_vu_source_tracks_selected_monitor_or_default_sink(self):
        app = object.__new__(padfire.PadfireApp)  # method only reads cfg; no UI/thread startup
        app.cfg = {"monitor_enabled": True, "monitor_sink": "vm_music", "default_sink": "vm_game"}
        self.assertEqual(app._vu_source(), "vm_music.monitor")
        app.cfg["monitor_enabled"] = False
        self.assertEqual(app._vu_source(), "vm_game.monitor")
        app.cfg["default_sink"] = "alsa_output.example.monitor"
        self.assertEqual(app._vu_source(), "alsa_output.example.monitor")
        app.cfg["default_sink"] = ""
        self.assertIsNone(app._vu_source())

    def test_all_led_states_keep_required_double_buffer_bits(self):
        for name in ("LED_OFF", "LED_DIM", "LED_GREEN", "LED_RED", "LED_ORANGE", "LED_AMBER", "LED_PG_HAS"):
            with self.subTest(name=name):
                self.assertEqual(getattr(padfire, name) & 0x0C, 0x0C)


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(TEST_HOME, ignore_errors=True)
