#!/usr/bin/env python3
"""Fast Padfire v2.3 regression checks; never opens MIDI/audio or real user config."""
import importlib.util
import json
import os
import shutil
import tempfile
import unittest
from types import SimpleNamespace
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

    def test_crosshair_pad_trigger_runs_cli_off_callback_thread_without_shell(self):
        app = object.__new__(padfire.PadfireApp)
        app._flash = lambda _row, _col: None
        app._get_pad = lambda _row, _col: {"action": "crosshair_toggle"}
        app._pad_key = lambda _row, _col: "0:17"
        workers = []

        class DeferredThread:
            def __init__(self, target, daemon=False):
                self.target = target
                self.daemon = daemon

            def start(self):
                workers.append(self)

            def run(self):
                self.target()

        with patch.object(padfire.threading, "Thread", DeferredThread), patch.object(
            padfire, "crosshair_toggle_argv",
            return_value=["/usr/bin/crosshair", "--toggle"],
        ), patch.object(
            padfire.subprocess, "run",
            return_value=SimpleNamespace(returncode=0, stderr="", stdout=""),
        ) as run:
            self.assertFalse(app._trigger_pad(1, 1))
            run.assert_not_called()
            self.assertEqual(len(workers), 1)
            self.assertTrue(workers[0].daemon)
            workers[0].run()
            run.assert_called_once_with(
                ["/usr/bin/crosshair", "--toggle"],
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )

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
