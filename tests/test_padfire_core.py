#!/usr/bin/env python3
"""Hardware-free Padfire core regressions; never opens real MIDI/audio or user config."""
import importlib.util
import json
import os
import shutil
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
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

    def test_padfire_shot_output_path_is_contained_and_rejects_unsafe_targets(self):
        from importlib.machinery import SourceFileLoader

        helper_path = ROOT / "tools" / "padfire-shot"
        loader = SourceFileLoader("padfire_shot_under_test", str(helper_path))
        helper_spec = importlib.util.spec_from_loader(loader.name, loader)
        self.assertIsNotNone(helper_spec)
        helper = importlib.util.module_from_spec(helper_spec)
        helper_spec.loader.exec_module(helper)

        screenshot_dir = TEST_HOME / "Pictures"
        screenshot_dir.mkdir()
        safe = helper.safe_output_path(screenshot_dir / "main.png")
        self.assertEqual(safe.parent, screenshot_dir)
        self.assertTrue(safe.parent.is_dir())
        with self.assertRaises(ValueError):
            helper.safe_output_path(TEST_HOME / "screenshots" / "main.png")
        with self.assertRaises(ValueError):
            helper.safe_output_path(screenshot_dir / "not-a-png.txt")
        with self.assertRaises(ValueError):
            helper.safe_output_path(TEST_HOME / ".config" / "nested.png")

        with self.assertRaises(ValueError):
            helper.safe_output_path("/etc/padfire-shot-forbidden.png")
        with self.assertRaises(ValueError):
            helper.safe_output_path(TEST_HOME / ".." / ".." / "etc" / "padfire-shot-forbidden.png")

        target = TEST_HOME / "existing-target.png"
        target.write_bytes(b"keep existing file")
        symlink = TEST_HOME / "output-symlink.png"
        symlink.symlink_to(target)
        with self.assertRaises(ValueError):
            helper.safe_output_path(symlink)

        nested_dir = TEST_HOME / ".config" / "nested"
        nested_dir.mkdir(parents=True)
        nested_target = nested_dir / "nested.png"
        nested_target.write_bytes(b"keep nested file")
        nested_link = TEST_HOME / "nested-link.png"
        nested_link.symlink_to(nested_target)
        with self.assertRaises(ValueError):
            helper.safe_output_path(nested_link)
        with self.assertRaises(ValueError):
            helper.safe_output_path(nested_target)

        outside_target = TEST_HOME / "outside-target.png"
        outside_target.symlink_to("/etc/passwd")
        with self.assertRaises(ValueError):
            helper.safe_output_path(outside_target)

        hardlink = TEST_HOME / "output-hardlink.png"
        os.link(target, hardlink)
        with self.assertRaises(ValueError):
            helper.safe_output_path(hardlink)

        external_parent = TEST_HOME / "external-parent"
        external_parent.symlink_to("/etc", target_is_directory=True)
        with self.assertRaises(ValueError):
            helper.safe_output_path(external_parent / "padfire-shot-forbidden.png")

        # A symlink inserted after the helper's own validation (while the
        # writer is producing the temporary file) is replaced as a directory
        # entry, never followed to overwrite its protected target.
        race_destination = TEST_HOME / "race.png"
        checked_destination = helper.safe_output_path(race_destination)
        protected_target = TEST_HOME / "protected.png"
        protected_target.write_bytes(b"preserve protected content")

        def add_destination_symlink(temporary):
            race_destination.symlink_to(protected_target)
            temporary.write_bytes(b"new screenshot")

        self.assertTrue(
            helper._write_atomically(checked_destination, add_destination_symlink)
        )
        self.assertFalse(race_destination.is_symlink())
        self.assertEqual(race_destination.read_bytes(), b"new screenshot")
        self.assertEqual(protected_target.read_bytes(), b"preserve protected content")

        self.assertEqual(target.read_bytes(), b"keep existing file")
        self.assertEqual(nested_target.read_bytes(), b"keep nested file")

    def test_release_version_changelog_and_license_are_consistent(self):
        changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
        license_text = (ROOT / "LICENSE").read_text(encoding="utf-8")
        readme = (ROOT / "README.md").read_text(encoding="utf-8")

        self.assertEqual(padfire.VER, "2.4.0")
        self.assertIn(f"## v{padfire.VER}", changelog)
        self.assertTrue(license_text.startswith("MIT License\n"))
        self.assertIn("Copyright (c) 2026 Roaring1", license_text)
        self.assertIn("[LICENSE](LICENSE)", readme)
        self.assertTrue((ROOT / "SECURITY.md").is_file())
        self.assertTrue((ROOT / "ROADMAP.md").is_file())
        self.assertTrue((ROOT / "CONTRIBUTING.md").is_file())
        self.assertTrue((ROOT / "RELEASING.md").is_file())

    def test_manual_launchpad_smoke_matches_supported_protocol_and_excludes_mk3(self):
        import importlib.util

        helper_path = ROOT / "packaging" / "extra-scripts" / "padfire_test.py"
        helper_spec = importlib.util.spec_from_file_location("padfire_manual_smoke", helper_path)
        self.assertIsNotNone(helper_spec)
        helper = importlib.util.module_from_spec(helper_spec)
        helper_spec.loader.exec_module(helper)

        self.assertTrue(helper.is_supported_port("Launchpad Mini MIDI 1"))
        self.assertTrue(helper.is_supported_port("Launchpad Mini MK2 MIDI 1"))
        self.assertFalse(helper.is_supported_port("Launchpad Mini MK3 MIDI 1"))
        self.assertFalse(helper.is_supported_port("Other MIDI Controller"))
        for color in (
            helper.LED_OFF, helper.LED_DIM, helper.LED_GREEN, helper.LED_RED,
            helper.LED_ORANGE, helper.LED_AMBER,
        ):
            self.assertEqual(color & 0x0C, 0x0C)

    def test_manual_launchpad_smoke_refuses_to_compete_with_live_padfire(self):
        import socket
        import json
        import importlib.util

        helper_path = ROOT / "packaging" / "extra-scripts" / "padfire_test.py"
        helper_spec = importlib.util.spec_from_file_location("padfire_manual_smoke_live", helper_path)
        helper = importlib.util.module_from_spec(helper_spec)
        helper_spec.loader.exec_module(helper)

        socket_path = TEST_HOME / "manual-smoke-live.sock"
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(socket_path))
        server.listen(1)

        def respond():
            conn, _ = server.accept()
            with conn:
                self.assertEqual(conn.recv(128), b"status\n")
                conn.sendall(json.dumps({"connected": True, "version": "2.4.0"}).encode() + b"\n")

        worker = threading.Thread(target=respond, daemon=True)
        worker.start()
        try:
            self.assertTrue(helper.padfire_is_running(socket_path))
        finally:
            server.close()
            worker.join(timeout=1)

    def test_manual_launchpad_smoke_refuses_non_socket_ipc_path(self):
        import importlib.util

        helper_path = ROOT / "packaging" / "extra-scripts" / "padfire_test.py"
        helper_spec = importlib.util.spec_from_file_location("padfire_manual_smoke_bad_path", helper_path)
        helper = importlib.util.module_from_spec(helper_spec)
        helper_spec.loader.exec_module(helper)

        path = TEST_HOME / "manual-smoke-not-a-socket"
        path.write_text("do not open or replace")
        self.assertIsNone(helper.padfire_is_running(path))
        self.assertEqual(path.read_text(), "do not open or replace")

    def test_restart_helper_gracefully_restarts_only_its_own_instance(self):
        import subprocess
        import tempfile
        import signal

        helper = ROOT / "packaging" / "extra-scripts" / "padfire-restart.sh"
        with tempfile.TemporaryDirectory(prefix="padfire-restart-test-") as raw_dir:
            temp_dir = Path(raw_dir)
            state_file = temp_dir / "dummy.pid"
            dummy_python = temp_dir / "dummy_padfire.py"
            dummy_launcher = temp_dir / "dummy_padfire-launch"
            log_file = temp_dir / "padfire.log"

            dummy_python.write_text(
                """import signal, sys, time
from pathlib import Path
state = Path(sys.argv[1])
state.write_text(str(__import__('os').getpid()))
def stop(*_):
    state.unlink(missing_ok=True)
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
while True:
    time.sleep(0.1)
""",
                encoding="utf-8",
            )
            dummy_launcher.write_text(
                """#!/usr/bin/env bash
set -euo pipefail
state="$PADFIRE_TEST_STATE"
case "${1:-}" in
  --status)
    if [[ -f "$state" ]]; then
      pid="$(cat "$state")"
      if kill -0 "$pid" 2>/dev/null; then
        echo '{"connected":false,"version":"test"}'
        exit 0
      fi
    fi
    echo '{"running":false}'
    ;;
  --quit)
    if [[ -f "$state" ]]; then kill "$(cat "$state")" 2>/dev/null || true; fi
    echo "ok"
    ;;
  *) exec python3 "$PADFIRE_PY" "$state" ;;
esac
""",
                encoding="utf-8",
            )
            dummy_launcher.chmod(0o755)

            env = os.environ.copy()
            env.update({
                "HOME": str(temp_dir),
                "PADFIRE_PY": str(dummy_python),
                "PADFIRE_LAUNCH": str(dummy_launcher),
                "PADFIRE_LOG": str(log_file),
                "PADFIRE_TEST_STATE": str(state_file),
            })
            sentinel = subprocess.Popen(["sleep", "30"])
            first = subprocess.Popen(
                [str(dummy_launcher)], env=env,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            restarted_pid = None
            try:
                deadline = padfire.time.monotonic() + 3
                while not state_file.exists() and padfire.time.monotonic() < deadline:
                    padfire.time.sleep(0.01)
                self.assertTrue(state_file.exists(), "dummy Padfire failed to start")

                result = subprocess.run(
                    ["bash", str(helper)], env=env, capture_output=True,
                    text=True, timeout=8, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Padfire is running.", result.stdout)
                self.assertTrue(state_file.exists(), "restart helper did not relaunch Padfire")
                restarted_pid = int(state_file.read_text())
                self.assertNotEqual(restarted_pid, first.pid)
                self.assertIsNone(sentinel.poll(), "restart helper affected an unrelated process")
                self.assertEqual(padfire.stat.S_IMODE(log_file.stat().st_mode), 0o600)

            finally:
                if state_file.exists():
                    try:
                        subprocess.run(
                            [str(dummy_launcher), "--quit"], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            timeout=2, check=False,
                        )
                    except subprocess.TimeoutExpired:
                        pass
                try:
                    first.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    first.terminate()
                    first.wait(timeout=2)
                if restarted_pid is not None:
                    try:
                        os.kill(restarted_pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                if sentinel.poll() is None:
                    sentinel.terminate()
                    sentinel.wait(timeout=2)

    def test_restart_helper_never_uses_broad_kill_or_unlinks_ipc_socket(self):
        helper = (ROOT / "packaging" / "extra-scripts" / "padfire-restart.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("--quit", helper)
        self.assertIn("is_running", helper)
        self.assertIn("refusing to force-kill", helper)
        self.assertIn("refusing to start a duplicate", helper)
        self.assertNotIn("pkill -f", helper)
        self.assertNotIn('rm -f "$SOCK"', helper)
        self.assertIn('chmod 600 "$LOG"', helper)

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

    def test_load_config_tightens_legacy_backup_permissions(self):
        primary = {"version": 5, "label": "current"}
        backup = {"version": 5, "label": "previous"}
        Path(padfire.CONF).write_text(json.dumps(primary), encoding="utf-8")
        Path(padfire.CONF + ".bak").write_text(json.dumps(backup), encoding="utf-8")
        Path(padfire.CONF).chmod(0o644)
        Path(padfire.CONF + ".bak").chmod(0o644)

        loaded = padfire.load_config()

        self.assertEqual(loaded["pages"], padfire.DEFAULT_CONFIG["pages"])
        self.assertEqual(json.loads(Path(padfire.CONF + ".bak").read_text()), backup)
        self.assertEqual(
            padfire.stat.S_IMODE(Path(padfire.CONF).stat().st_mode), 0o600
        )
        self.assertEqual(
            padfire.stat.S_IMODE(Path(padfire.CONF + ".bak").stat().st_mode), 0o600
        )

    def test_config_and_backup_are_private_after_save(self):
        first = {"version": 5, "command": "example --argument"}
        second = {"version": 5, "command": "updated --argument"}
        Path(padfire.CONF).write_text(json.dumps(first), encoding="utf-8")
        Path(padfire.CONF).chmod(0o644)
        padfire.CFGDIR.chmod(0o755)

        padfire.save_config(second)

        self.assertEqual(padfire.stat.S_IMODE(padfire.CFGDIR.stat().st_mode), 0o700)
        self.assertEqual(padfire.stat.S_IMODE(Path(padfire.CONF).stat().st_mode), 0o600)
        self.assertEqual(
            padfire.stat.S_IMODE(Path(padfire.CONF + ".bak").stat().st_mode), 0o600
        )
        self.assertEqual(json.loads(Path(padfire.CONF).read_text()), second)
        self.assertEqual(json.loads(Path(padfire.CONF + ".bak").read_text()), first)
        self.assertEqual(padfire.load_config()["command"], "updated --argument")

    def test_config_directory_refuses_symlinked_directory(self):
        target = self.config_dir / "real-config"
        target.mkdir()
        link = self.config_dir / "linked-config"
        link.symlink_to(target, target_is_directory=True)

        with patch.object(padfire, "CFGDIR", link):
            with self.assertRaises(PermissionError):
                padfire._ensure_private_config_dir()

        self.assertTrue(target.is_dir())
        self.assertTrue(link.is_symlink())

    def test_config_file_size_is_bounded(self):
        path = Path(padfire.CONF)
        with open(path, "wb") as stream:
            stream.truncate(padfire.MAX_CONFIG_BYTES + 1)

        config = padfire.load_config()

        self.assertEqual(config["version"], padfire.CONFIG_VERSION)
        self.assertEqual(path.stat().st_size, padfire.MAX_CONFIG_BYTES + 1)

    def test_save_refuses_oversized_config_without_replacing_valid_file(self):
        valid = {"version": 5, "label": "valid config"}
        padfire.save_config(valid)
        previous = Path(padfire.CONF).read_bytes()
        too_large = {
            "version": 5,
            "pages": [{} for _ in range(8)],
            "notes": "x" * (padfire.MAX_CONFIG_BYTES + 1),
        }

        with self.assertRaises(ValueError):
            padfire.save_config(too_large)

        self.assertEqual(Path(padfire.CONF).read_bytes(), previous)

    def test_load_config_refuses_special_files_without_blocking(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("FIFO security check is POSIX-only")
        os.mkfifo(padfire.CONF)
        config = padfire.load_config()
        self.assertEqual(config["version"], padfire.CONFIG_VERSION)
        self.assertTrue(Path(padfire.CONF).is_fifo())

    def test_load_config_refuses_hardlinked_config_without_chmodding_other_path(self):
        target = self.config_dir / "shared.json"
        target.write_text(json.dumps({"version": 5, "label": "shared"}), encoding="utf-8")
        target.chmod(0o644)
        os.link(target, Path(padfire.CONF))

        config = padfire.load_config()

        self.assertEqual(config["version"], padfire.CONFIG_VERSION)
        self.assertEqual(padfire.stat.S_IMODE(target.stat().st_mode), 0o644)
        self.assertEqual(target.stat().st_nlink, 2)
        self.assertEqual(json.loads(target.read_text())["label"], "shared")

    def test_load_config_refuses_symlinked_config_file(self):
        target = self.config_dir / "outside.json"
        target.write_text(json.dumps({"version": 5, "secret": "must not load"}))
        Path(padfire.CONF).symlink_to(target)

        config = padfire.load_config()

        self.assertEqual(config["version"], padfire.CONFIG_VERSION)
        self.assertNotIn("secret", config)
        self.assertTrue(Path(padfire.CONF).is_symlink())
        self.assertIn("secret", json.loads(target.read_text()))

    def test_save_config_replaces_symlink_without_modifying_target(self):
        target = self.config_dir / "outside.json"
        original = {"version": 5, "secret": "keep this untouched"}
        target.write_text(json.dumps(original))
        Path(padfire.CONF).symlink_to(target)
        replacement = {"version": 5, "pages": [{} for _ in range(8)]}

        padfire.save_config(replacement)

        self.assertFalse(Path(padfire.CONF).is_symlink())
        self.assertEqual(json.loads(Path(padfire.CONF).read_text()), replacement)
        self.assertEqual(json.loads(target.read_text()), original)
        self.assertEqual(padfire.stat.S_IMODE(Path(padfire.CONF).stat().st_mode), 0o600)

    def test_ipc_request_reader_accepts_bounded_newline_command(self):
        left, right = padfire.socket.socketpair()
        try:
            command = "x" * padfire.IPC_MAX_REQUEST
            right.sendall((command + "\n").encode())
            self.assertEqual(padfire._read_ipc_request(left), command)
        finally:
            left.close()
            right.close()

    def test_ipc_request_reader_rejects_oversized_or_unterminated_input(self):
        left, right = padfire.socket.socketpair()
        try:
            right.sendall(b"x" * (padfire.IPC_MAX_REQUEST + 1))
            self.assertIsNone(padfire._read_ipc_request(left))
        finally:
            left.close()
            right.close()

        left, right = padfire.socket.socketpair()
        try:
            right.sendall(b"status")
            right.shutdown(padfire.socket.SHUT_WR)
            self.assertIsNone(padfire._read_ipc_request(left))
        finally:
            left.close()
            right.close()

    def test_ipc_server_uses_private_socket_and_does_not_evict_live_instance(self):
        socket_path = TEST_HOME / "ipc-live.sock"
        refs = []
        second_refs = []
        with patch.object(padfire, "SOCK", str(socket_path)):
            server = threading.Thread(
                target=padfire.ipc_serve, args=(lambda cmd: "reply:" + cmd, refs),
                daemon=True,
            )
            server.start()
            deadline = padfire.time.monotonic() + 2
            while not socket_path.exists() and padfire.time.monotonic() < deadline:
                padfire.time.sleep(0.01)
            self.assertTrue(socket_path.exists())
            self.assertEqual(padfire.stat.S_IMODE(socket_path.stat().st_mode), 0o600)

            second = threading.Thread(
                target=padfire.ipc_serve, args=(lambda cmd: "wrong", second_refs),
                daemon=True,
            )
            second.start()
            second.join(timeout=0.5)
            self.assertFalse(second.is_alive())
            self.assertEqual(second_refs, [])
            self.assertEqual(padfire.ipc_send("status", str(socket_path)), "reply:status")

            refs[0].close()
            server.join(timeout=2)
            self.assertFalse(server.is_alive())

    def test_ipc_server_never_unlinks_a_regular_file_at_socket_path(self):
        socket_path = TEST_HOME / "ipc.sock"
        socket_path.write_text("preserve this file")
        refs = []
        with patch.object(padfire, "SOCK", str(socket_path)):
            padfire.ipc_serve(lambda _cmd: "ok", refs)
        self.assertEqual(socket_path.read_text(), "preserve this file")
        self.assertEqual(refs, [])

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

    def test_system_default_selection_is_distinct_from_global_sink(self):
        choices = [None, "output.main", "output.headphones"]
        self.assertEqual(padfire._sink_from_selection(choices, 0), "")
        self.assertEqual(padfire._sink_from_selection(choices, 1), "output.main")
        self.assertEqual(padfire._sink_from_selection(choices, 2), "output.headphones")
        self.assertEqual(padfire._sink_from_selection(choices, -1), "")
        self.assertEqual(padfire._sink_from_selection(choices, 99), "")

    def test_new_install_defaults_to_audio_server_output(self):
        self.assertEqual(padfire.DEFAULT_CONFIG["default_sink"], "")
        self.assertEqual(padfire.DEFAULT_CONFIG["monitor_sink"], "")

    def test_audio_sink_resolution_falls_back_when_saved_sink_is_missing(self):
        engine = object.__new__(padfire.AudioEngine)
        engine._sinks = ["output.main", "output.secondary"]
        self.assertEqual(engine._find_sink(""), "")
        self.assertEqual(engine._find_sink("missing.saved.device"), "")
        self.assertEqual(engine._find_sink("output.secondary"), "output.secondary")

    def test_paplay_uses_server_default_without_empty_device_argument(self):
        self.assertEqual(
            padfire.AudioEngine._paplay_argv("", 32768, "/tmp/test.wav"),
            ["paplay", "--volume=32768", "/tmp/test.wav"],
        )
        self.assertEqual(
            padfire.AudioEngine._paplay_argv("output.main", 32768, "/tmp/test.wav"),
            ["paplay", "--device=output.main", "--volume=32768", "/tmp/test.wav"],
        )

    def test_system_default_monitor_mirrors_only_when_route_differs(self):
        class DeferredThread:
            def __init__(self, target, args=(), daemon=None, name=None):
                self.target = target
                self.args = args
                self.name = name

            def start(self):
                workers.append(self)

        for pad_sink, monitor_sink, expected_workers in (
            ("output.main", "", 2),
            ("", "", 1),
            ("output.main", None, 1),
        ):
            with self.subTest(
                pad_sink=pad_sink, monitor_sink=monitor_sink
            ):
                engine = object.__new__(padfire.AudioEngine)
                engine._procs = {}
                engine._generations = {}
                engine._lock = threading.Lock()
                engine._sinks = ["output.main", "output.headphones"]
                workers = []
                with patch.object(padfire.threading, "Thread", DeferredThread):
                    engine.play(
                        "0:9", "/tmp/test.wav", pad_sink, 80, False,
                        monitor_sink=monitor_sink,
                    )
                self.assertEqual(len(workers), expected_workers)
                if expected_workers == 2:
                    self.assertEqual(workers[1].args[0], "__mon__0:9")
                    self.assertEqual(workers[1].args[1], "")

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
                engine._sinks = ["test_sink"]
                workers = []
                with patch.object(padfire.threading, "Thread", DeferredThread):
                    with patch.object(padfire.subprocess, "Popen") as popen:
                        engine.play("0:1", "/tmp/sound.wav", "test_sink", 80, False)
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
        engine._sinks = ["test_sink"]
        workers = []
        with patch.object(padfire.threading, "Thread", DeferredThread):
            engine.play("0:2", "/tmp/old.wav", "test_sink", 80, False)
            engine.play("0:2", "/tmp/new.wav", "test_sink", 80, False)
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
        engine._sinks = ["test_sink"]
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
                engine.play("0:3", "/tmp/racing.wav", "test_sink", 80, False)
                self.assertTrue(entered_popen.wait(1), "playback worker never reached Popen")
                engine.stop("0:3")
                release_popen.set()
                self.assertTrue(process.killed.wait(1), "late subprocess survived Stop")
                workers[0].join(timeout=1)
        self.assertFalse(engine.is_playing("0:3"))

    def _bare_midi_engine(self):
        engine = object.__new__(padfire.MidiEngine)
        engine._in = engine._out = None
        engine._input_name = engine._output_name = None
        engine._last_presence_error = 0.0
        engine._out_lock = threading.Lock()
        engine._conn_lock = threading.Lock()
        engine._connected = False
        engine._running = True
        engine._stop_event = threading.Event()
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

    def test_midi_presence_check_disconnects_when_usb_ports_disappear(self):
        class FakePort:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        input_port = FakePort()
        output_port = FakePort()
        engine._in = input_port
        engine._out = output_port
        engine._input_name = "Launchpad Mini MIDI 1"
        engine._output_name = "Launchpad Mini MIDI 1"
        engine._connected = True
        disconnect_events = []
        engine._on_disconnect = lambda: disconnect_events.append(True)

        with patch.object(padfire.mido, "get_input_names", return_value=[]), patch.object(
            padfire.mido, "get_output_names", return_value=[]
        ):
            engine._check_connected_device()

        self.assertFalse(engine.is_connected())
        self.assertTrue(input_port.closed)
        self.assertTrue(output_port.closed)
        self.assertIsNone(engine._input_name)
        self.assertIsNone(engine._output_name)
        self.assertEqual(disconnect_events, [True])

    def test_midi_presence_check_keeps_connection_on_transient_scan_error(self):
        class FakePort:
            closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        input_port = FakePort()
        output_port = FakePort()
        engine._in = input_port
        engine._out = output_port
        engine._input_name = "Launchpad Mini MIDI 1"
        engine._output_name = "Launchpad Mini MIDI 1"
        engine._connected = True

        with patch.object(
            padfire.mido, "get_input_names", side_effect=OSError("ALSA restarting")
        ):
            engine._check_connected_device()

        self.assertTrue(engine.is_connected())
        self.assertFalse(input_port.closed)
        self.assertFalse(output_port.closed)

    def test_midi_reader_disconnects_when_input_iterator_ends_normally(self):
        engine = self._bare_midi_engine()
        input_port = type("EndedInput", (), {
            "closed": False,
            "__iter__": lambda _self: iter(()),
            "close": lambda self: setattr(self, "closed", True),
        })()
        output_port = type("FakeOutput", (), {
            "closed": False,
            "close": lambda self: setattr(self, "closed", True),
        })()
        engine._in = input_port
        engine._out = output_port
        engine._input_name = engine._output_name = "Launchpad Mini MIDI 1"
        engine._connected = True
        engine._on_disconnect = lambda: engine._stop_event.set()

        engine._read_loop()

        self.assertFalse(engine.is_connected())
        self.assertTrue(input_port.closed)
        self.assertTrue(output_port.closed)

    def test_midi_reconnect_watcher_checks_presence_while_connected(self):
        engine = self._bare_midi_engine()
        engine._connected = True

        class ContinueUntilReconnect:
            def __init__(self):
                self.waits = []

            def wait(self, seconds):
                self.waits.append(seconds)
                return False

        stop = ContinueUntilReconnect()
        engine._stop_event = stop

        def device_went_away():
            engine._connected = False

        def reconnect_attempt():
            engine._running = False

        with patch.object(
            engine, "_check_connected_device", side_effect=device_went_away
        ) as check, patch.object(
            engine, "try_connect", side_effect=reconnect_attempt
        ) as reconnect:
            engine._reconnect_loop()

        self.assertEqual(stop.waits, [1.0, 1.0])
        check.assert_called_once()
        reconnect.assert_called_once()

    def test_midi_watcher_reconnects_after_simulated_usb_unplug_and_replug(self):
        class FakePort:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        engine = self._bare_midi_engine()
        old_input = FakePort()
        old_output = FakePort()
        new_input = FakePort()
        new_output = FakePort()
        engine._in = old_input
        engine._out = old_output
        engine._input_name = engine._output_name = "Launchpad Mini MIDI 1"
        engine._connected = True

        events = []
        engine._on_disconnect = lambda: events.append("disconnected")

        def reconnected():
            events.append("connected")
            engine._running = False

        engine._on_connect = reconnected
        stop = type("Continue", (), {"wait": lambda _self, _seconds: False})()
        engine._stop_event = stop

        with patch.object(
            padfire.mido, "get_input_names",
            side_effect=[[], ["Launchpad Mini MIDI 2"]],
        ), patch.object(
            padfire.mido, "get_output_names",
            side_effect=[[], ["Launchpad Mini MIDI 2"]],
        ), patch.object(
            padfire.mido, "open_output", return_value=new_output
        ), patch.object(
            padfire.mido, "open_input", return_value=new_input
        ):
            engine._reconnect_loop()

        self.assertTrue(old_input.closed)
        self.assertTrue(old_output.closed)
        self.assertTrue(engine.is_connected())
        self.assertIs(engine._in, new_input)
        self.assertIs(engine._out, new_output)
        self.assertEqual(engine._input_name, "Launchpad Mini MIDI 2")
        self.assertEqual(engine._output_name, "Launchpad Mini MIDI 2")
        self.assertEqual(events, ["disconnected", "connected"])

    def test_midi_status_label_says_connected_or_disconnected(self):
        class FakeStyleContext:
            def __init__(self):
                self.classes = set()

            def remove_class(self, name):
                self.classes.discard(name)

            def add_class(self, name):
                self.classes.add(name)

        class FakeLabel:
            def __init__(self):
                self.text = ""
                self.tooltip = ""
                self.context = FakeStyleContext()

            def set_text(self, value):
                self.text = value

            def set_tooltip_text(self, value):
                self.tooltip = value

            def get_style_context(self):
                return self.context

        class FakeMidi:
            connected = True

            def is_connected(self):
                return self.connected

        app = object.__new__(padfire.PadfireApp)
        app.midi = FakeMidi()
        app._midi_lbl = FakeLabel()

        app._update_midi_lbl()
        self.assertEqual(app._midi_lbl.text, "● Connected")
        self.assertIn("connected", app._midi_lbl.tooltip.lower())
        self.assertIn("midi-ok", app._midi_lbl.context.classes)

        app.midi.connected = False
        app._update_midi_lbl()
        self.assertEqual(app._midi_lbl.text, "○ Disconnected")
        self.assertIn("disconnected", app._midi_lbl.tooltip.lower())
        self.assertIn("midi-bad", app._midi_lbl.context.classes)

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

    def test_editor_test_run_skips_launch_after_dialog_cancellation(self):
        cancelled = threading.Event()
        cancelled.set()
        with patch.object(padfire.subprocess, "Popen") as popen:
            padfire._run_script_test_command("echo test", cancelled)
        popen.assert_not_called()

    def test_editor_test_run_terminates_process_created_after_dialog_close(self):
        cancelled = threading.Event()
        process = self._fake_process()

        def spawn(*_args, **_kwargs):
            cancelled.set()
            return process

        with patch.object(padfire.subprocess, "Popen", side_effect=spawn):
            padfire._run_script_test_command("test-command", cancelled)

        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertTrue(process.killed)

    def test_editor_test_run_timeout_escalates_its_process_group(self):
        cancelled = threading.Event()
        process = self._fake_process()
        process.pid = 54321

        with patch.object(
            padfire.subprocess, "Popen", return_value=process
        ) as popen, patch.object(padfire.os, "killpg") as killpg:
            padfire._run_script_test_command("slow-test-command", cancelled, timeout=0)

        self.assertTrue(popen.call_args.kwargs["start_new_session"])
        self.assertEqual(
            killpg.call_args_list,
            [
                ((54321, padfire.signal.SIGTERM),),
                ((54321, padfire.signal.SIGKILL),),
            ],
        )
        self.assertTrue(process.waited)

    def test_lufs_analysis_skips_process_creation_after_cancellation(self):
        cancelled = threading.Event()
        cancelled.set()
        with patch.object(padfire.subprocess, "Popen") as popen:
            self.assertIsNone(padfire.analyze_lufs("unused.wav", cancelled))
        popen.assert_not_called()

    def test_lufs_analysis_terminates_ffmpeg_if_dialog_closes_during_startup(self):
        cancelled = threading.Event()
        process = self._fake_process()

        def spawn(*_args, **_kwargs):
            cancelled.set()
            return process

        with patch.object(padfire.subprocess, "Popen", side_effect=spawn):
            self.assertIsNone(padfire.analyze_lufs("unused.wav", cancelled))

        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        if os.name == "posix":
            self.assertTrue(process.killed)

    def test_lufs_analysis_timeout_reaps_ffmpeg(self):
        process = self._fake_process()
        with patch.object(padfire.subprocess, "Popen", return_value=process):
            self.assertIsNone(padfire.analyze_lufs("unused.wav", timeout=0))
        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)

    def test_lufs_analysis_parses_ffmpeg_measurement(self):
        stderr = (
            "noise before json\\n"
            '{"input_i":"-20.0","input_tp":"-1.0"}'
        )
        process = self._fake_process(lambda: ("", stderr))
        with patch.object(padfire.subprocess, "Popen", return_value=process):
            result = padfire.analyze_lufs("unused.wav")
        self.assertEqual(result["input_i"], -20.0)
        self.assertEqual(result["input_tp"], -1.0)
        self.assertGreaterEqual(result["suggested_vol"], 1)
        self.assertLessEqual(result["suggested_vol"], 150)

    def test_editor_preview_rejects_cancelled_dialog(self):
        audio = unittest.mock.Mock()
        cancelled = threading.Event()
        cancelled.set()
        lock = threading.Lock()
        stop_pending = threading.Event()
        generation = [3]

        self.assertFalse(padfire._play_editor_preview(
            audio, "__test__", "clip.wav", "test_sink", 80, False,
            cancelled, lock, 3, generation, stop_pending,
        ))
        audio.play.assert_not_called()

    def test_editor_preview_rejects_stale_generation_after_stop(self):
        audio = unittest.mock.Mock()
        cancelled = threading.Event()
        lock = threading.Lock()
        stop_pending = threading.Event()
        generation = [4]

        self.assertFalse(padfire._play_editor_preview(
            audio, "__test__", "clip.wav", "test_sink", 80, False,
            cancelled, lock, 3, generation, stop_pending,
        ))
        audio.play.assert_not_called()

    def test_editor_preview_snapshots_and_passes_values_when_current(self):
        audio = unittest.mock.Mock()
        cancelled = threading.Event()
        lock = threading.Lock()
        stop_pending = threading.Event()
        generation = [3]

        self.assertTrue(padfire._play_editor_preview(
            audio, "__test__", "clip.wav", "monitor_output", 72, True,
            cancelled, lock, 3, generation, stop_pending,
        ))
        audio.play.assert_called_once_with(
            "__test__", "clip.wav", "monitor_output", 72, True
        )

    def test_editor_preview_cancel_waits_for_pending_worker_lock(self):
        audio = unittest.mock.Mock()
        cancelled = threading.Event()
        lock = threading.Lock()
        stop_pending = threading.Event()
        generation = [1]
        entered = threading.Event()
        lock.acquire()
        worker = threading.Thread(
            target=lambda: (
                entered.set(),
                padfire._play_editor_preview(
                    audio, "__test__", "clip.wav", "test_sink", 80, False,
                    cancelled, lock, 1, generation, stop_pending,
                ),
            )
        )
        worker.start()
        self.assertTrue(entered.wait(1))
        cancelled.set()
        generation[0] += 1
        lock.release()
        worker.join(timeout=1)

        self.assertFalse(worker.is_alive())
        audio.play.assert_not_called()

    def test_executable_path_is_quoted_only_when_needed(self):
        self.assertEqual(
            padfire._quote_executable("/home/user/bin/padfire-launch"),
            "/home/user/bin/padfire-launch",
        )
        self.assertEqual(
            padfire._quote_executable("/home/user name/bin/padfire-launch"),
            '"/home/user name/bin/padfire-launch"',
        )

    def test_desktop_installer_bundles_icon_and_rewrites_launcher_paths(self):
        home = TEST_HOME / self._testMethodName
        launcher = home / "bin dir" / "padfire-launch"
        launcher.parent.mkdir(parents=True, exist_ok=True)
        launcher.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        launcher.chmod(0o755)

        autostart = home / ".config" / "autostart" / "padfire.desktop"
        autostart.parent.mkdir(parents=True, exist_ok=True)
        autostart.write_text(
            "[Desktop Entry]\n"
            "Exec=/old/home/bin/padfire-launch\n"
            "Icon=/old/home/padfire.png\n"
            "[Desktop Action quit]\n"
            "Exec=/old/home/bin/padfire-launch --quit\n",
            encoding="utf-8",
        )

        original_home = padfire.HOME
        try:
            padfire.HOME = home
            with patch.object(padfire.shutil, "which", return_value=None):
                desktop = padfire.install_desktop_integration(launcher)
        finally:
            padfire.HOME = original_home

        self.assertEqual(desktop, home / ".local/share/applications/padfire.desktop")
        installed = desktop.read_text(encoding="utf-8")
        quoted_launcher = f'"{launcher}"'
        self.assertIn(f"Exec={quoted_launcher}\n", installed)
        self.assertIn(f"TryExec={quoted_launcher}\n", installed)
        self.assertIn("Icon=padfire\n", installed)
        self.assertIn(f"Exec={quoted_launcher} --quit\n", installed)
        self.assertNotIn("/old/home", installed)

        icon = home / ".local/share/icons/hicolor/scalable/apps/padfire.svg"
        self.assertEqual(
            icon.read_bytes(),
            (ROOT / "packaging/icons/hicolor/scalable/apps/padfire.svg").read_bytes(),
        )
        autostart_text = autostart.read_text(encoding="utf-8")
        self.assertIn(f"Exec={quoted_launcher} --quit\n", autostart_text)
        self.assertIn("Icon=padfire\n", autostart_text)
        self.assertFalse(desktop.with_name("padfire.desktop.tmp").exists())
        self.assertFalse(icon.with_name("padfire.svg.tmp").exists())

    def test_desktop_installer_does_not_create_autostart(self):
        home = TEST_HOME / self._testMethodName
        launcher = home / "bin" / "padfire-launch"
        launcher.parent.mkdir(parents=True, exist_ok=True)
        launcher.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        launcher.chmod(0o755)
        autostart = home / ".config" / "autostart" / "padfire.desktop"
        original_home = padfire.HOME
        try:
            padfire.HOME = home
            with patch.object(padfire.shutil, "which", return_value=None):
                self.assertIsNotNone(padfire.install_desktop_integration(launcher))
        finally:
            padfire.HOME = original_home
        self.assertFalse(autostart.exists())

    def test_user_service_orders_after_pipewire_pulse(self):
        required_after = (
            "After=graphical-session.target pipewire.service "
            "pipewire-pulse.service wireplumber.service"
        )
        required_wants = "Wants=pipewire.service pipewire-pulse.service wireplumber.service"
        sources = (
            (ROOT / "packaging" / "padfire.service").read_text(encoding="utf-8"),
            (ROOT / "padfire.py").read_text(encoding="utf-8"),
        )
        for source in sources:
            with self.subTest(source="service template"):
                self.assertIn(required_after, source)
                self.assertIn(required_wants, source)

    def test_padfire_has_no_hearth_runtime_coupling_or_footer_widgets(self):
        source = (ROOT / "padfire.py").read_text(encoding="utf-8")
        self.assertNotIn("Hearth", source)
        self.assertNotIn("rac.sock", source)
        self.assertNotIn("_start_rac_probe", source)
        self.assertNotIn("_rac_thread", source)
        self.assertNotIn("statusbar", source)
        self.assertLess(source.index('brand.pack_start(logo'), source.index('brand.pack_start(self._vu_bar, '))
        self.assertLess(source.index('topbar.pack_start(brand'), source.index('topbar.pack_start(self._play_lbl,'))

    def _bare_app(self):
        app = object.__new__(padfire.PadfireApp)

        class FakeAudio:
            def __init__(self):
                self._lock = threading.Lock()
                self._procs = {}
                self.stopped = False

            def stop(self, _key):
                pass

            def stop_all(self):
                self.stopped = True

        app.audio = FakeAudio()
        app.midi = None
        app._shutdown_event = threading.Event()
        app._script_procs = {}
        app._script_generations = {}
        app._script_threads = set()
        app._script_reapers = set()
        app._proc_lock = threading.Lock()
        app._led_dirty = False
        app._vu_level = 0.0
        app._vu_thread = None
        app._ipc_ref = []
        app._test_pad = {"action": "script", "cmd": "test-script"}
        app._flash = lambda _row, _col: None
        app._get_pad = lambda _row, _col: app._test_pad
        app._pad_key = lambda _row, _col: "0:0"
        app._vu_source = lambda: "test_sink.monitor"
        return app

    def _fake_process(self, communicate=None):
        class FakeProcess:
            def __init__(self):
                self.returncode = None
                self.terminated = False
                self.killed = False
                self.waited = False

            def poll(self):
                return self.returncode

            def terminate(self):
                self.terminated = True
                self.returncode = -15

            def kill(self):
                self.killed = True
                self.returncode = -9

            def wait(self, timeout=None):
                self.waited = True
                return self.returncode

            def communicate(self, timeout=None):
                return communicate() if communicate else ("", None)

        return FakeProcess()

    def test_stop_cancels_script_still_starting_in_popen(self):
        app = self._bare_app()
        entered_popen = threading.Event()
        release_popen = threading.Event()
        process = self._fake_process()

        def delayed_popen(*_args, **_kwargs):
            entered_popen.set()
            if not release_popen.wait(2):
                raise TimeoutError("test did not release script Popen")
            return process

        with patch.object(padfire.subprocess, "Popen", side_effect=delayed_popen):
            app._trigger_pad(0, 0)
            self.assertTrue(entered_popen.wait(1), "script worker did not reach Popen")
            with app._proc_lock:
                workers = list(app._script_threads)
            app._stop_key("0:0")
            release_popen.set()
            for worker in workers:
                worker.join(timeout=2)

        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertNotIn("0:0", app._script_procs)
        self.assertFalse(app._script_threads)

    def test_stop_all_cancels_script_still_starting_in_popen(self):
        app = self._bare_app()
        entered_popen = threading.Event()
        release_popen = threading.Event()
        process = self._fake_process()

        def delayed_popen(*_args, **_kwargs):
            entered_popen.set()
            if not release_popen.wait(2):
                raise TimeoutError("test did not release script Popen")
            return process

        with patch.object(padfire.subprocess, "Popen", side_effect=delayed_popen):
            app._trigger_pad(0, 0)
            self.assertTrue(entered_popen.wait(1), "script worker did not reach Popen")
            with app._proc_lock:
                workers = list(app._script_threads)
            app._stop_script_processes()
            release_popen.set()
            for worker in workers:
                worker.join(timeout=2)

        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertNotIn("0:0", app._script_procs)
        self.assertFalse(app._script_threads)

    def test_quit_during_script_popen_terminates_late_process(self):
        app = self._bare_app()
        entered_popen = threading.Event()
        release_popen = threading.Event()
        process = self._fake_process()

        def delayed_popen(*_args, **_kwargs):
            entered_popen.set()
            if not release_popen.wait(2):
                raise TimeoutError("test did not release script Popen")
            return process

        with patch.object(padfire.subprocess, "Popen", side_effect=delayed_popen), patch.object(
            padfire.Gtk, "main_quit"
        ):
            app._trigger_pad(0, 0)
            self.assertTrue(entered_popen.wait(1), "script worker did not reach Popen")

            def release_when_shutting_down():
                if app._shutdown_event.wait(1):
                    release_popen.set()

            releaser = threading.Thread(target=release_when_shutting_down)
            releaser.start()
            app._quit()
            releaser.join(timeout=1)

        self.assertFalse(releaser.is_alive())
        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertFalse(app._script_threads)
        self.assertNotIn("0:0", app._script_procs)

    def test_script_retrigger_keeps_new_process_registered_when_old_worker_finishes(self):
        app = self._bare_app()
        release_old = threading.Event()
        release_new = threading.Event()
        old_entered = threading.Event()
        new_entered = threading.Event()

        old_process = self._fake_process(
            lambda: (old_entered.set(), release_old.wait(2), ("", None))[-1]
        )
        new_process = self._fake_process(
            lambda: (new_entered.set(), release_new.wait(2), ("", None))[-1]
        )

        def open_script(command, **_kwargs):
            return old_process if command == "old-script" else new_process

        with patch.object(padfire.subprocess, "Popen", side_effect=open_script):
            app._test_pad = {"action": "script", "cmd": "old-script"}
            app._trigger_pad(0, 0)
            self.assertTrue(old_entered.wait(1), "old script did not start")
            with app._proc_lock:
                old_workers = list(app._script_threads)

            app._test_pad = {"action": "script", "cmd": "new-script"}
            app._trigger_pad(0, 0)
            self.assertTrue(new_entered.wait(1), "new script did not start")
            self.assertTrue(old_process.terminated)
            with app._proc_lock:
                self.assertIs(app._script_procs.get("0:0"), new_process)
                new_workers = list(app._script_threads)

            release_old.set()
            for worker in old_workers:
                worker.join(timeout=1)
            with app._proc_lock:
                self.assertIs(
                    app._script_procs.get("0:0"),
                    new_process,
                    "the old worker must not remove the newer process",
                )

            app._stop_script_processes()
            release_new.set()
            for worker in new_workers:
                worker.join(timeout=1)

        self.assertFalse(app._script_threads)
        self.assertNotIn("0:0", app._script_procs)

    def test_script_termination_targets_only_its_dedicated_process_group(self):
        process = self._fake_process()
        process.pid = 43210

        with patch.object(padfire.os, "killpg") as killpg:
            padfire._terminate_process(process, process_group=True)

        self.assertEqual(
            killpg.call_args_list,
            [
                ((43210, padfire.signal.SIGTERM),),
                ((43210, padfire.signal.SIGKILL),),
            ],
        )
        self.assertTrue(process.waited)

    def test_script_process_group_kills_descendant_that_ignores_sigterm(self):
        if os.name != "posix" or not Path("/proc").is_dir():
            self.skipTest("requires Linux process groups and /proc")

        code = (
            "import subprocess,sys,time;"
            "child=subprocess.Popen([sys.executable,'-c',"
            "\"import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(30)\"]);"
            "print(child.pid,flush=True); time.sleep(30)"
        )
        process = padfire.subprocess.Popen(
            [padfire.sys.executable, "-c", code],
            stdout=padfire.subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        try:
            line = process.stdout.readline()
            self.assertTrue(line.strip(), "child process did not report its PID")
            child_pid = int(line.strip())
            padfire._terminate_process(process, timeout=0.5, process_group=True)
            self.assertIsNotNone(process.poll(), "script process was not reaped")

            state = None
            for _ in range(40):
                try:
                    state = Path(f"/proc/{child_pid}/stat").read_text().split()[2]
                except FileNotFoundError:
                    state = None
                if state is None or state == "Z":
                    break
                padfire.time.sleep(0.025)
            self.assertIn(
                state, (None, "Z"),
                "child that ignored SIGTERM survived the process-group SIGKILL",
            )
        finally:
            if process.poll() is None:
                padfire._terminate_process(process, process_group=True)
            if process.stdout is not None:
                process.stdout.close()

    def test_vu_shutdown_terminates_reaps_and_closes_parec_pipe(self):
        app = self._bare_app()
        app.audio._procs = {"0:1": object()}
        process_started = threading.Event()

        class FakePipe:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        process = self._fake_process()
        process.stdout = FakePipe()

        def open_parec(*_args, **_kwargs):
            process_started.set()
            return process

        def wait_for_shutdown(*_args, **_kwargs):
            app._shutdown_event.wait(0.01)
            return [], [], []

        with patch.object(padfire.shutil, "which", return_value="/usr/bin/parec"), patch.object(
            padfire.subprocess, "Popen", side_effect=open_parec
        ), patch.object(padfire.select, "select", side_effect=wait_for_shutdown):
            app._start_vu()
            self.assertTrue(process_started.wait(1), "VU monitor did not start parec")
            app._shutdown_event.set()
            app._vu_thread.join(timeout=2)

        self.assertFalse(app._vu_thread.is_alive())
        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertTrue(process.stdout.closed)
        self.assertEqual(app._vu_level, 0.0)

    def test_quit_waits_for_pending_async_script_reaper(self):
        app = self._bare_app()
        process = self._fake_process()
        app._script_procs["0:1"] = process
        reaper_entered = threading.Event()
        release_reaper = threading.Event()

        def blocked_reap(_processes):
            reaper_entered.set()
            release_reaper.wait(2)

        with patch.object(
            app, "_reap_script_processes", side_effect=blocked_reap
        ), patch.object(padfire.Gtk, "main_quit"):
            app._stop_script_processes(("0:1",), wait=False)
            self.assertTrue(reaper_entered.wait(1), "async reaper did not start")
            with app._proc_lock:
                reapers = list(app._script_reapers)
            self.assertEqual(len(reapers), 1)

            def release_when_shutting_down():
                if app._shutdown_event.wait(1):
                    release_reaper.set()

            releaser = threading.Thread(target=release_when_shutting_down)
            releaser.start()
            app._quit()
            releaser.join(timeout=1)
            for worker in reapers:
                worker.join(timeout=1)

        self.assertFalse(releaser.is_alive())
        self.assertFalse(any(worker.is_alive() for worker in reapers))
        self.assertFalse(app._script_reapers)

    def test_quit_is_idempotent_and_stops_tracked_scripts(self):
        app = self._bare_app()
        process = self._fake_process()
        app._script_procs["0:1"] = process

        class FakeMidi:
            def __init__(self):
                self.stopped = False

            def shutdown(self):
                self.stopped = True

        app.midi = FakeMidi()
        with patch.object(padfire.Gtk, "main_quit") as main_quit:
            app._quit()
            app._quit()

        self.assertTrue(app._shutdown_event.is_set())
        self.assertTrue(app.audio.stopped)
        self.assertTrue(app.midi.stopped)
        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        main_quit.assert_called_once_with()

    def test_stop_all_invalidates_script_generation_before_worker_runs(self):
        app = self._bare_app()
        app._script_generations["0:0"] = 4
        app._test_pad = {"action": "stop_all"}
        workers = []

        class DeferredThread:
            def __init__(self, target, daemon=False, **_kwargs):
                self.target = target

            def start(self):
                workers.append(self)

            def run(self):
                self.target()

        with patch.object(padfire.threading, "Thread", DeferredThread):
            app._trigger_pad(0, 0)
            self.assertEqual(app._script_generations["0:0"], 5)
            self.assertFalse(app.audio.stopped)
            self.assertEqual(len(workers), 1)
            workers[0].run()

        self.assertTrue(app.audio.stopped)

    def test_vu_retains_odd_pcm_byte_between_pipe_reads(self):
        app = self._bare_app()
        app.audio._procs = {"0:1": object()}
        second_chunk = threading.Event()
        third_read = threading.Event()
        release_third_read = threading.Event()

        class FakePipe:
            def __init__(self):
                self.calls = 0
                self.closed = False

            def read(self, _size):
                self.calls += 1
                if self.calls == 1:
                    return b"\\x00"
                if self.calls == 2:
                    second_chunk.set()
                    return b"\\x01"
                if self.calls == 3:
                    third_read.set()
                    release_third_read.wait(1)
                    return b""
                return b""

            def close(self):
                self.closed = True

        class FakeProcess:
            def __init__(self):
                self.returncode = None
                self.stdout = FakePipe()
                self.terminated = False
                self.waited = False

            def poll(self):
                return self.returncode

            def terminate(self):
                self.terminated = True
                self.returncode = -15

            def kill(self):
                self.returncode = -9

            def wait(self, timeout=None):
                self.waited = True
                return self.returncode

        process = FakeProcess()
        with patch.object(padfire.shutil, "which", return_value="/usr/bin/parec"), patch.object(
            padfire.subprocess, "Popen", return_value=process
        ), patch.object(
            padfire.select, "select", side_effect=lambda ready, _w, _x, _t: (ready, [], [])
        ):
            app._start_vu()
            self.assertTrue(third_read.wait(1), "VU reader did not request the next PCM chunk")
            deadline = padfire.time.monotonic() + 1
            while app._vu_level <= 0 and padfire.time.monotonic() < deadline:
                padfire.time.sleep(0.001)
            self.assertGreater(
                app._vu_level, 0.0,
                "two one-byte reads should combine into one non-zero s16le sample",
            )
            app._shutdown_event.set()
            release_third_read.set()
            app._vu_thread.join(timeout=2)

        self.assertFalse(app._vu_thread.is_alive())
        self.assertTrue(process.terminated)
        self.assertTrue(process.waited)
        self.assertTrue(process.stdout.closed)

    def test_midi_try_connect_refuses_after_shutdown(self):
        engine = object.__new__(padfire.MidiEngine)
        engine._conn_lock = threading.Lock()
        engine._connected = False
        engine._running = False

        with patch.object(padfire.mido, "get_input_names") as get_inputs:
            self.assertFalse(engine.try_connect())
            get_inputs.assert_not_called()

    def test_midi_reconnect_loop_does_not_reconnect_after_shutdown(self):
        engine = object.__new__(padfire.MidiEngine)
        engine._running = True
        engine._connected = False

        class StopDuringWait:
            def wait(self, _seconds):
                engine._running = False
                return True

        engine._stop_event = StopDuringWait()
        with patch.object(engine, "try_connect") as try_connect:
            engine._reconnect_loop()

        try_connect.assert_not_called()

    def test_midi_reader_exits_when_idle_stop_event_is_set(self):
        engine = self._bare_midi_engine()

        class StopImmediately:
            def __init__(self):
                self.waited = []

            def is_set(self):
                return False

            def wait(self, timeout):
                self.waited.append(timeout)
                return True

        stop = StopImmediately()
        engine._stop_event = stop
        engine._read_loop()
        self.assertEqual(stop.waited, [0.05])

    def test_midi_shutdown_closes_port_and_unblocks_reader_thread(self):
        engine = object.__new__(padfire.MidiEngine)
        engine._running = True
        engine._connected = True
        engine._stop_event = threading.Event()
        engine._conn_lock = threading.Lock()
        engine._out_lock = threading.Lock()
        iterator_started = threading.Event()
        release_iterator = threading.Event()

        class BlockingInput:
            closed = False

            def __iter__(self):
                iterator_started.set()
                release_iterator.wait(2)
                return iter(())

            def close(self):
                self.closed = True
                release_iterator.set()

        class FakeOutput:
            closed = False

            def send(self, _message):
                pass

            def close(self):
                self.closed = True

        input_port = BlockingInput()
        output_port = FakeOutput()
        engine._in = input_port
        engine._out = output_port
        engine._on_disconnect = lambda: None
        engine._read_thread = threading.Thread(target=engine._read_loop)
        engine._reconnect_thread = threading.Thread(target=engine._reconnect_loop)
        engine._read_thread.start()
        engine._reconnect_thread.start()

        self.assertTrue(iterator_started.wait(1), "MIDI reader did not enter port iteration")
        engine.shutdown()

        self.assertTrue(input_port.closed)
        self.assertTrue(output_port.closed)
        self.assertFalse(engine._read_thread.is_alive())
        self.assertFalse(engine._reconnect_thread.is_alive())

    def test_midi_shutdown_wakes_and_joins_both_workers(self):
        engine = self._bare_midi_engine()

        class FakeThread:
            def __init__(self):
                self.join_timeouts = []

            def join(self, timeout=None):
                self.join_timeouts.append(timeout)

        reader = FakeThread()
        watcher = FakeThread()
        engine._read_thread = reader
        engine._reconnect_thread = watcher

        engine.shutdown()

        self.assertFalse(engine._running)
        self.assertTrue(engine._stop_event.is_set())
        self.assertEqual(reader.join_timeouts, [1.0])
        self.assertEqual(watcher.join_timeouts, [1.0])

    def test_vu_source_tracks_selected_monitor_or_default_sink(self):
        app = object.__new__(padfire.PadfireApp)  # method only reads cfg; no UI/thread startup
        class FakeAudio:
            available = {"monitor_output", "test_sink", "alsa_output.example"}

            def _find_sink(self, name):
                return name if name in self.available else ""

        app.audio = FakeAudio()
        app._system_default_sink_cache = ""
        app._system_default_sink_checked = 0.0
        app.cfg = {"monitor_enabled": True, "monitor_sink": "monitor_output", "default_sink": "test_sink"}
        self.assertEqual(app._vu_source(), "monitor_output.monitor")
        app.cfg["monitor_enabled"] = False
        self.assertEqual(app._vu_source(), "test_sink.monitor")
        app.cfg["default_sink"] = "alsa_output.example.monitor"
        self.assertEqual(app._vu_source(), "alsa_output.example.monitor")
        app.cfg["default_sink"] = ""
        with patch.object(
            padfire.subprocess, "check_output", return_value=b"output.from.server.default"
        ) as check_output:
            self.assertEqual(app._vu_source(), "output.from.server.default.monitor")
        check_output.assert_called_once_with(
            ["pactl", "get-default-sink"],
            stderr=padfire.subprocess.DEVNULL,
            timeout=3,
        )
        with patch.object(padfire.subprocess, "check_output") as cached_lookup:
            self.assertEqual(app._vu_source(), "output.from.server.default.monitor")
        cached_lookup.assert_not_called()

        app._system_default_sink_checked -= 6.0
        with patch.object(padfire.subprocess, "check_output", side_effect=OSError):
            self.assertIsNone(app._vu_source())

        app.cfg["monitor_enabled"] = True
        app.cfg["monitor_sink"] = "missing.monitor"
        app._system_default_sink_cache = ""
        app._system_default_sink_checked = 0.0
        with patch.object(
            padfire.subprocess, "check_output", return_value=b"current.system.output"
        ):
            self.assertEqual(app._vu_source(), "current.system.output.monitor")

    def test_first_default_sink_lookup_is_not_delayed_by_boot_uptime(self):
        app = object.__new__(padfire.PadfireApp)
        app.cfg = {"monitor_enabled": False, "default_sink": ""}
        app.audio = type("Audio", (), {"_find_sink": lambda _self, _name: ""})()
        app._system_default_sink_cache = ""
        app._system_default_sink_checked = None
        with patch.object(padfire.time, "monotonic", return_value=2.0):
            with patch.object(
                padfire.subprocess, "check_output", return_value=b"fresh.default.output"
            ) as check_output:
                self.assertEqual(
                    app._vu_source(), "fresh.default.output.monitor"
                )
        check_output.assert_called_once()
        self.assertEqual(app._system_default_sink_checked, 2.0)

    def test_idle_vu_worker_does_not_probe_or_spawn_audio_processes(self):
        app = object.__new__(padfire.PadfireApp)

        class FakeAudio:
            def __init__(self):
                self._lock = threading.Lock()
                self._procs = {}

        class StopAfterOneIdleWait:
            def __init__(self):
                self.checks = 0
                self.wait_timeouts = []

            def is_set(self):
                self.checks += 1
                return self.checks > 1

            def wait(self, timeout):
                self.wait_timeouts.append(timeout)
                return False

        class ImmediateThread:
            def __init__(self, target, **_kwargs):
                self.target = target

            def start(self):
                self.target()

        app.audio = FakeAudio()
        app._shutdown_event = StopAfterOneIdleWait()
        app._vu_level = 0.0
        app._vu_source = lambda: self.fail("idle VU worker probed pactl")
        with patch.object(padfire.shutil, "which", return_value="/usr/bin/parec"):
            with patch.object(padfire.threading, "Thread", ImmediateThread):
                with patch.object(padfire.subprocess, "Popen") as popen:
                    app._start_vu()
        popen.assert_not_called()
        self.assertEqual(app._vu_level, 0.0)
        self.assertEqual(app._shutdown_event.wait_timeouts, [0.25])

    def test_all_led_states_keep_required_double_buffer_bits(self):
        for name in ("LED_OFF", "LED_DIM", "LED_GREEN", "LED_RED", "LED_ORANGE", "LED_AMBER", "LED_PG_HAS"):
            with self.subTest(name=name):
                self.assertEqual(getattr(padfire, name) & 0x0C, 0x0C)


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(TEST_HOME, ignore_errors=True)
