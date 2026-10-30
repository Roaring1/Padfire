"""Offscreen Qt UI smoke tests; no MIDI hardware or audio device is opened."""
from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

from PySide6.QtWidgets import QApplication

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


if __name__ == "__main__":
    unittest.main(verbosity=2)
