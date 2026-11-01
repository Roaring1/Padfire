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
        app._rac_thread = None
        app._ipc_ref = []
        app._test_pad = {"action": "script", "cmd": "test-script"}
        app._flash = lambda _row, _col: None
        app._get_pad = lambda _row, _col: app._test_pad
        app._pad_key = lambda _row, _col: "0:0"
        app._vu_source = lambda: "vm_game.monitor"
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

    def test_rac_probe_exits_promptly_when_shutdown_is_signalled(self):
        app = self._bare_app()
        probed = threading.Event()

        def fake_rac_send(_command):
            probed.set()
            return "{}"

        with patch.object(padfire, "rac_send", side_effect=fake_rac_send), patch.object(
            padfire.GLib, "idle_add"
        ):
            app._start_rac_probe()
            self.assertTrue(probed.wait(1), "Hearth probe did not run")
            app._shutdown_event.set()
            app._rac_thread.join(timeout=1)

        self.assertFalse(app._rac_thread.is_alive())

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

        def shutdown_during_wait(_seconds):
            engine._running = False

        with patch.object(
            padfire.time, "sleep", side_effect=shutdown_during_wait
        ), patch.object(engine, "try_connect") as try_connect:
            engine._reconnect_loop()

        try_connect.assert_not_called()

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
