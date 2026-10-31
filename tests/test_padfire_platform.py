"""Cross-platform tests for the shared config, MIDI mapping and local IPC."""
from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
import sys
import subprocess
from unittest.mock import patch
from unittest.mock import patch

from padfire_core import (
    CONFIG_VERSION, PadfirePaths, default_config, get_paths, load_config_file,
    migrate_config, migrate_legacy_config, save_config_file, validate_pad_config,
)
from padfire_win.ipc import IPCServer, ipc_send
from padfire_win.midi import MidiEngine, note_side, note_xy
import padfire_win.app as windows_app
from padfire_win.app import command_for_platform, install_startup


class ConfigPortTests(unittest.TestCase):
    def test_windows_paths_use_roaming_config_and_short_socket(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            appdata = Path(temp) / "Roaming"
            local = Path(temp) / "Local"
            with patch.dict(os.environ, {"APPDATA": str(appdata), "LOCALAPPDATA": str(local)}):
                paths = get_paths(windows=True)
        self.assertEqual(paths.config_dir, appdata / "Padfire")
        self.assertEqual(paths.config_file, appdata / "Padfire" / "padfire.json")
        self.assertIsNone(paths.hearth_socket)
        self.assertLess(len(str(paths.socket_file)), 100)
        self.assertTrue(str(paths.pid_file).startswith(str(local)))

    def test_windows_default_config_uses_system_audio_default(self) -> None:
        config = default_config(windows=True)
        self.assertEqual(config["version"], CONFIG_VERSION)
        self.assertEqual(config["default_sink"], "")
        self.assertFalse(config["monitor_enabled"])
        self.assertEqual(len(config["pages"]), 8)

    def test_v3_config_migrates_without_losing_pad_assignments(self) -> None:
        original = {
            "version": 3,
            "device": "obsolete",
            "retrigger": "toggle",
            "pages": [{"0": {"file": "sound.wav", "mode": "old"}}],
        }
        migrated = migrate_config(original)
        self.assertEqual(migrated["version"], CONFIG_VERSION)
        self.assertEqual(migrated["retrigger"], "toggle")
        self.assertEqual(migrated["pages"][0]["0"]["action"], "play")
        self.assertNotIn("device", migrated)
        self.assertNotIn("mode", migrated["pages"][0]["0"])
        self.assertEqual(len(migrated["pages"]), 8)

    def test_broken_config_falls_back_and_atomic_backup_is_retained(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            config_file = Path(temp) / "Padfire" / "padfire.json"
            config_file.parent.mkdir()
            config_file.write_text("{ broken", encoding="utf-8")
            fallback = load_config_file(config_file, windows=True)
            self.assertEqual(fallback["version"], CONFIG_VERSION)
            self.assertEqual(fallback["default_sink"], "")

            first = default_config(windows=True)
            first["pages"][0]["0"] = {"action": "play", "file": "old.wav"}
            second = default_config(windows=True)
            second["pages"][0]["1"] = {"action": "script", "cmd": "example.cmd"}
            save_config_file(first, config_file)
            save_config_file(second, config_file)
            current = json.loads(config_file.read_text(encoding="utf-8"))
            self.assertEqual(set(current["pages"][0]), {"1"})
            backup = json.loads(Path(str(config_file) + ".bak").read_text(encoding="utf-8"))
            self.assertIn("0", backup["pages"][0])
            self.assertFalse(list(config_file.parent.glob("*.tmp")))

    def test_legacy_config_is_copied_only_when_destination_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / ".config"
            source = root / "roaring" / "padfire.json"
            destination = root / "padfire" / "padfire.json"
            source.parent.mkdir(parents=True)
            source.write_text('{"version": 5, "pages": []}', encoding="utf-8")
            self.assertTrue(migrate_legacy_config(destination))
            self.assertTrue(destination.is_file())
            destination.write_text('{"version": 5, "pages": []}', encoding="utf-8")
            self.assertFalse(migrate_legacy_config(destination))

    def test_validator_reports_malformed_pages(self) -> None:
        config = default_config(windows=True)
        config["pages"][0] = "not-a-page"
        self.assertTrue(validate_pad_config(config))


class PythonPadCommandTests(unittest.TestCase):
    def test_source_checkout_uses_its_active_python_interpreter(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            script = Path(temp) / "sample pad.py"
            script.write_text("print('ok')", encoding="utf-8")
            command = subprocess.list2cmdline([str(script)]) + " --flag"
            with patch.object(sys, "frozen", False, create=True):
                launch, use_shell = command_for_platform(command)
        self.assertEqual(launch, [sys.executable, str(script), "--flag"])
        self.assertFalse(use_shell)

    def test_frozen_build_uses_an_external_python_interpreter(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            script = Path(temp) / "sample.py"
            script.write_text("print('ok')", encoding="utf-8")
            with (
                patch.object(sys, "frozen", True, create=True),
                patch("padfire_win.app.shutil.which", return_value=r"C:\Python312\python.exe") as which,
            ):
                launch, use_shell = command_for_platform(str(script))
        which.assert_called_once_with("python.exe")
        self.assertEqual(launch, [r"C:\Python312\python.exe", str(script)])
        self.assertFalse(use_shell)

    def test_frozen_build_reports_missing_python_interpreter(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            script = Path(temp) / "sample.py"
            script.write_text("print('ok')", encoding="utf-8")
            with (
                patch.object(sys, "frozen", True, create=True),
                patch("padfire_win.app.shutil.which", return_value=None),
            ):
                with self.assertRaisesRegex(FileNotFoundError, "requires python.exe or py.exe"):
                    command_for_platform(str(script))


class StartupRegistrationTests(unittest.TestCase):
    def test_install_creates_run_key_and_registers_gui_from_cli(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cli_dir = root / "CLI"
            cli_dir.mkdir()
            cli_exe = cli_dir / "PadfireCLI.exe"
            gui_exe = root / "Padfire.exe"
            cli_exe.touch()
            gui_exe.touch()

            key_handle = unittest.mock.MagicMock()
            opened_key = key_handle.__enter__.return_value
            registry = type("FakeWinreg", (), {
                "HKEY_CURRENT_USER": object(),
                "KEY_SET_VALUE": 2,
                "REG_SZ": 1,
                "CreateKeyEx": unittest.mock.MagicMock(return_value=key_handle),
                "OpenKey": unittest.mock.MagicMock(),
                "SetValueEx": unittest.mock.MagicMock(),
                "DeleteValue": unittest.mock.MagicMock(),
            })()
            fake_sys = type("FakeSys", (), {
                "frozen": True,
                "executable": str(cli_exe),
                "stderr": sys.stderr,
            })()

            with (
                patch.object(windows_app, "os", type("FakeOS", (), {"name": "nt"})()),
                patch.object(windows_app, "sys", fake_sys),
                patch.dict(sys.modules, {"winreg": registry}),
            ):
                result = install_startup(True)

            self.assertEqual(result, 0)
            registry.CreateKeyEx.assert_called_once_with(
                registry.HKEY_CURRENT_USER,
                r"Software\Microsoft\Windows\CurrentVersion\Run",
                0,
                registry.KEY_SET_VALUE,
            )
            registry.OpenKey.assert_not_called()
            registry.SetValueEx.assert_called_once_with(
                opened_key, "Padfire", 0, registry.REG_SZ, f'"{gui_exe.resolve()}"'
            )

    def test_uninstall_succeeds_when_run_key_does_not_exist(self) -> None:
        registry = type("FakeWinreg", (), {
            "HKEY_CURRENT_USER": object(),
            "KEY_SET_VALUE": 2,
            "REG_SZ": 1,
            "CreateKeyEx": unittest.mock.MagicMock(),
            "OpenKey": unittest.mock.MagicMock(side_effect=FileNotFoundError),
            "SetValueEx": unittest.mock.MagicMock(),
            "DeleteValue": unittest.mock.MagicMock(),
        })()
        fake_sys = type("FakeSys", (), {
            "frozen": True,
            "executable": "PadfireCLI.exe",
            "stderr": sys.stderr,
        })()

        with (
            patch.object(windows_app, "os", type("FakeOS", (), {"name": "nt"})()),
            patch.object(windows_app, "sys", fake_sys),
            patch.dict(sys.modules, {"winreg": registry}),
        ):
            result = install_startup(False)

        self.assertEqual(result, 0)
        registry.OpenKey.assert_called_once()
        registry.CreateKeyEx.assert_not_called()
        registry.DeleteValue.assert_not_called()


class MidiContractTests(unittest.TestCase):
    def test_launchpad_note_layout_and_mk3_exclusion_are_unchanged(self) -> None:
        self.assertEqual(note_xy(7, 7), 119)
        self.assertEqual(note_side(7), 120)
        self.assertEqual(
            MidiEngine._find(["Launchpad Mini MK3 MIDI 1", "Launchpad Mini MIDI 1"]),
            "Launchpad Mini MIDI 1",
        )
        self.assertIsNone(MidiEngine._find(["Launchpad Mini MK3 MIDI 1", "Other"]))


class LocalIpcTests(unittest.TestCase):
    def test_single_instance_control_pipe_round_trip(self) -> None:
        base = Path(tempfile.gettempdir()) / f"padfire-ipc-{os.getpid()}"
        paths = PadfirePaths(
            config_dir=base,
            config_file=base / "padfire.json",
            socket_file=(
                Path(f"Padfire-test-{os.getpid()}")
                if os.name == "nt"
                else Path(tempfile.gettempdir()) / f"pf-test-{os.getpid()}.sock"
            ),
            pid_file=base / "padfire.pid",
            hearth_socket=None,
        )
        server = IPCServer(lambda command: json.dumps({"command": command}), paths)
        try:
            server.start()
            reply = ipc_send("status", paths)
            self.assertEqual(json.loads(reply or "{}"), {"command": "status"})
        finally:
            server.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
