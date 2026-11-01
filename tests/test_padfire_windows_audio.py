"""Hardware-free Windows audio lifecycle tests.

These tests mock file decoding and WASAPI streams; they never open an audio
device or require a Launchpad.
"""
from __future__ import annotations

import threading
import unittest
from unittest.mock import patch

import numpy as np

from padfire_win import audio


RealThread = threading.Thread


class IdleStream:
    """A minimal OutputStream context manager that immediately becomes idle."""

    def __init__(
        self,
        entered: threading.Event | None = None,
        exited: threading.Event | None = None,
    ) -> None:
        self.active = False
        self.aborted = False
        self.entered = entered
        self.exited = exited

    def __enter__(self) -> "IdleStream":
        if self.entered is not None:
            self.entered.set()
        return self

    def __exit__(self, *_args: object) -> None:
        if self.exited is not None:
            self.exited.set()

    def abort(self) -> None:
        self.aborted = True


class WindowsAudioLifecycleTests(unittest.TestCase):
    def make_engine(self) -> audio.AudioEngine:
        engine = audio.AudioEngine.__new__(audio.AudioEngine)
        engine.disabled = False
        engine._lock = threading.RLock()
        engine._streams = {}
        engine._generations = {}
        engine._sinks = []
        engine._level = 0.0
        return engine

    def test_stop_and_stop_all_cancel_file_load_before_stream_start(self) -> None:
        for stop_all in (False, True):
            with self.subTest(stop_all=stop_all):
                engine = self.make_engine()
                read_entered = threading.Event()
                release_read = threading.Event()
                output_stream = patch.object(audio.sd, "OutputStream")

                def read_file(*_args: object, **_kwargs: object):
                    read_entered.set()
                    if not release_read.wait(2):
                        raise TimeoutError("test did not release audio file read")
                    return np.ones((32, 1), dtype=np.float32), 48000

                worker = RealThread(
                    target=engine.play,
                    args=("0:1", "delayed.wav", None, 80, False),
                )
                with patch.object(audio.sf, "read", side_effect=read_file), output_stream as create_stream, patch.object(
                    engine, "_resolve_device", return_value=0
                ):
                    worker.start()
                    self.assertTrue(read_entered.wait(1), "play did not enter file decoding")
                    if stop_all:
                        engine.stop_all()
                    else:
                        engine.stop("0:1")
                    release_read.set()
                    worker.join(timeout=2)
                    self.assertFalse(worker.is_alive(), "play worker did not finish")
                    create_stream.assert_not_called()
                self.assertFalse(engine.is_playing("0:1"))

    def test_newer_play_supersedes_an_older_slow_file_load(self) -> None:
        engine = self.make_engine()
        old_read_entered = threading.Event()
        release_old_read = threading.Event()
        new_stream_entered = threading.Event()
        new_stream_exited = threading.Event()
        sample_data = np.ones((32, 1), dtype=np.float32)
        created_streams: list[IdleStream] = []

        def read_file(filepath: str, **_kwargs: object):
            if filepath == "old.wav":
                old_read_entered.set()
                if not release_old_read.wait(2):
                    raise TimeoutError("test did not release old audio file read")
            return sample_data, 48000

        def make_stream(**_kwargs: object) -> IdleStream:
            stream = IdleStream(entered=new_stream_entered, exited=new_stream_exited)
            created_streams.append(stream)
            return stream

        older_play = RealThread(
            target=engine.play,
            args=("0:2", "old.wav", None, 80, False),
        )
        with patch.object(audio.sf, "read", side_effect=read_file), patch.object(
            audio.sd, "OutputStream", side_effect=make_stream
        ) as create_stream, patch.object(engine, "_resolve_device", return_value=0):
            older_play.start()
            self.assertTrue(old_read_entered.wait(1), "older play did not enter file decoding")
            engine.play("0:2", "new.wav", None, 80, False)
            self.assertTrue(new_stream_entered.wait(1), "newer playback did not open its stream")
            self.assertTrue(new_stream_exited.wait(1), "newer playback stream did not close")
            release_old_read.set()
            older_play.join(timeout=2)
            self.assertFalse(older_play.is_alive(), "older play worker did not finish")
            self.assertEqual(create_stream.call_count, 1)

        self.assertEqual(len(created_streams), 1)
        self.assertFalse(engine.is_playing("0:2"))

    def test_stop_during_stream_open_aborts_the_late_stream(self) -> None:
        engine = self.make_engine()
        stream_entered = threading.Event()
        release_stream_open = threading.Event()
        stream_aborted = threading.Event()

        class DelayedStream(IdleStream):
            def __enter__(self) -> "DelayedStream":
                stream_entered.set()
                if not release_stream_open.wait(2):
                    raise TimeoutError("test did not release stream initialization")
                return self

            def abort(self) -> None:
                super().abort()
                stream_aborted.set()

        def read_file(*_args: object, **_kwargs: object):
            return np.ones((32, 1), dtype=np.float32), 48000

        def make_delayed_stream(**_kwargs: object) -> DelayedStream:
            return DelayedStream()

        with patch.object(audio.sf, "read", side_effect=read_file), patch.object(
            audio.sd, "OutputStream", side_effect=make_delayed_stream
        ), patch.object(engine, "_resolve_device", return_value=0):
            worker = RealThread(
                target=engine.play,
                args=("0:3", "stream.wav", None, 80, False),
            )
            worker.start()
            self.assertTrue(stream_entered.wait(1), "audio stream did not begin opening")
            engine.stop("0:3")
            release_stream_open.set()
            worker.join(timeout=2)
            self.assertFalse(worker.is_alive(), "play worker did not finish")

        self.assertTrue(stream_aborted.wait(1), "late-opened audio stream was not aborted")
        self.assertFalse(engine.is_playing("0:3"))

    def test_stop_cannot_observe_a_registered_unstarted_thread(self) -> None:
        engine = self.make_engine()
        engine._generations["0:4"] = 1
        start_entered = threading.Event()
        release_start = threading.Event()
        stream_entered = threading.Event()
        stop_finished = threading.Event()
        stop_errors: list[BaseException] = []

        class GatedThread:
            def __init__(self, *args: object, **kwargs: object) -> None:
                self._thread = RealThread(*args, **kwargs)

            @property
            def ident(self):
                return self._thread.ident

            def start(self) -> None:
                start_entered.set()
                if not release_start.wait(2):
                    raise TimeoutError("test did not release Thread.start")
                self._thread.start()

            def join(self, timeout: float | None = None) -> None:
                self._thread.join(timeout)

        def stop() -> None:
            try:
                engine.stop("0:4")
            except BaseException as exc:  # capture RuntimeError from joining an unstarted thread
                stop_errors.append(exc)
            finally:
                stop_finished.set()

        with patch.object(audio.threading, "Thread", GatedThread), patch.object(
            audio.sd, "OutputStream", side_effect=lambda **_kwargs: IdleStream(stream_entered)
        ):
            publisher = RealThread(
                target=engine._start_stream,
                args=("0:4", np.ones((16, 1), dtype=np.float32), 48000, 0, 0.8, False),
                kwargs={"generation": 1},
            )
            publisher.start()
            self.assertTrue(start_entered.wait(1), "stream publisher did not reach Thread.start")
            stopper = RealThread(target=stop)
            stopper.start()
            self.assertFalse(
                stop_finished.wait(0.05),
                "Stop should wait for atomic registration/start instead of observing an unstarted thread",
            )
            release_start.set()
            publisher.join(timeout=2)
            stopper.join(timeout=2)

        self.assertFalse(publisher.is_alive())
        self.assertFalse(stopper.is_alive())
        self.assertEqual(stop_errors, [])
        self.assertFalse(engine.is_playing("0:4"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
