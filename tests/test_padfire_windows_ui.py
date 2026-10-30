"""Offscreen Qt UI smoke tests; no MIDI hardware or audio device is opened."""
from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from PySide6.QtWidgets import QApplication, QPushButton

from padfire_win.app import PadfireWindow


class WindowsUiSmokeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.app = QApplication.instance() or QApplication(["padfire-ui-test"])

    def test_window_builds_all_pages_and_pad_cells_without_hardware(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            appdata = Path(temp) / "Roaming"
            local = Path(temp) / "Local"
            old_appdata = os.environ.get("APPDATA")
            old_local = os.environ.get("LOCALAPPDATA")
            os.environ["APPDATA"] = str(appdata)
            os.environ["LOCALAPPDATA"] = str(local)
            window = PadfireWindow(enable_hardware=False, start_ipc=False)
            try:
                # The config exists immediately after first-run construction.
                self.assertTrue((appdata / "Padfire" / "padfire.json").is_file())
                self.assertEqual(len(window._pad_buttons), 72)
                self.assertEqual(len(window._page_buttons), 8)
                self.assertEqual(window.cfg["version"], 5)
                window._set_pad(0, 0, {"action": "stop_all", "label": "Stop All"})
                self.assertEqual(window._pad(0, 0)["action"], "stop_all")
                self.assertTrue((appdata / "Padfire" / "padfire.json").is_file())
                self.assertTrue(window._status()["running"])
                window.show()
                self.app.processEvents()
            finally:
                window._quitting = True
                window.audio.stop_all()
                window.close()
                if old_appdata is None:
                    os.environ.pop("APPDATA", None)
                else:
                    os.environ["APPDATA"] = old_appdata
                if old_local is None:
                    os.environ.pop("LOCALAPPDATA", None)
                else:
                    os.environ["LOCALAPPDATA"] = old_local

    def test_about_button_and_qt_lgpl_notice_are_visible(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            appdata = Path(temp) / "Roaming"
            local = Path(temp) / "Local"
            old_appdata = os.environ.get("APPDATA")
            old_local = os.environ.get("LOCALAPPDATA")
            os.environ["APPDATA"] = str(appdata)
            os.environ["LOCALAPPDATA"] = str(local)
            window = PadfireWindow(enable_hardware=False, start_ipc=False)
            try:
                about = window.findChild(QPushButton, "aboutButton")
                self.assertIsNotNone(about)
                self.assertEqual(about.text(), "About")
                with patch("padfire_win.app.QMessageBox.about") as show_about:
                    window._show_about()
                self.assertTrue(show_about.called)
                message = show_about.call_args.args[2]
                self.assertIn("LGPLv3", message)
                self.assertIn("LGPL-3.0-and-GPL-3.0.txt", message)
                self.assertIn("COMPONENTS.md", message)
            finally:
                window._quitting = True
                window.audio.stop_all()
                window.close()
                if old_appdata is None:
                    os.environ.pop("APPDATA", None)
                else:
                    os.environ["APPDATA"] = old_appdata
                if old_local is None:
                    os.environ.pop("LOCALAPPDATA", None)
                else:
                    os.environ["LOCALAPPDATA"] = old_local

    def test_timer_reconciles_launchpad_led_after_playback_state_changes(self) -> None:
        from padfire_core import LED_DIM, LED_GREEN

        with tempfile.TemporaryDirectory() as temp:
            appdata = Path(temp) / "Roaming"
            local = Path(temp) / "Local"
            old_appdata = os.environ.get("APPDATA")
            old_local = os.environ.get("LOCALAPPDATA")
            os.environ["APPDATA"] = str(appdata)
            os.environ["LOCALAPPDATA"] = str(local)
            window = PadfireWindow(enable_hardware=False, start_ipc=False)

            class FakeMidi:
                def __init__(self) -> None:
                    self.notes: list[tuple[int, int]] = []

                def is_connected(self) -> bool:
                    return True

                def led(self, note: int, color: int) -> None:
                    self.notes.append((note, color))

                def cc_led(self, control: int, color: int) -> None:
                    pass

            midi = FakeMidi()
            try:
                window.cfg["pages"][0]["0"] = {"action": "play", "file": "missing.wav"}
                window.midi = midi  # hardware protocol is replaced by a recording fake
                window._last_leds["n:0"] = LED_GREEN
                window._led_dirty = False
                window._tick()
                self.assertIn((0, LED_DIM), midi.notes)
            finally:
                window.midi = None
                window._quitting = True
                window.audio.stop_all()
                window.close()
                if old_appdata is None:
                    os.environ.pop("APPDATA", None)
                else:
                    os.environ["APPDATA"] = old_appdata
                if old_local is None:
                    os.environ.pop("LOCALAPPDATA", None)
                else:
                    os.environ["LOCALAPPDATA"] = old_local


if __name__ == "__main__":
    unittest.main(verbosity=2)
