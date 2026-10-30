from __future__ import annotations
import multiprocessing.connection as mpconn
import os
import socket
import threading
from pathlib import Path
from typing import Callable
from padfire_core import PadfirePaths, get_paths

_AUTHKEY = b"padfire-local-v1"

def _pipe_name(value: Path | str) -> str:
    name = str(value)
    prefix = "\\\\.\\pipe\\"
    return name if name.startswith(prefix) else prefix + name.lstrip("\\/")

def ipc_send(command: str, paths: PadfirePaths | None = None, timeout: float = 0.75) -> str | None:
    target = paths or get_paths()
    if os.name == "nt":
        conn = None
        try:
            conn = mpconn.Client(_pipe_name(target.socket_file), family="AF_PIPE", authkey=_AUTHKEY)
            conn.send_bytes(command.strip().encode("utf-8"))
            return conn.recv_bytes(65536).decode("utf-8", errors="replace")
        except (OSError, EOFError, mpconn.AuthenticationError, TimeoutError):
            return None
        finally:
            if conn is not None:
                conn.close()
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        client.settimeout(timeout)
        client.connect(str(target.socket_file))
        client.sendall((command.strip() + "\n").encode("utf-8"))
        data = bytearray()
        while len(data) < 65536:
            chunk = client.recv(min(4096, 65536 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
            if b"\n" in chunk:
                break
        return bytes(data).split(b"\n", 1)[0].decode("utf-8", errors="replace")
    except (OSError, TimeoutError):
        return None
    finally:
        client.close()

class IPCServer:
    def __init__(self, handler: Callable[[str], str | None], paths: PadfirePaths | None = None) -> None:
        self.paths = paths or get_paths()
        self.handler = handler
        self._server = None
        self._pipe_listener = None
        self._stop = threading.Event()
        self._thread = None
        self._ready = threading.Event()
        self._startup_error = None

    def start(self) -> None:
        if os.name == "nt":
            self._thread = threading.Thread(target=self._serve_windows, name="padfire-ipc", daemon=True)
            self._thread.start()
            if not self._ready.wait(3.0):
                raise RuntimeError("Timed out starting Padfire IPC.")
            if self._startup_error:
                raise RuntimeError(self._startup_error)
            return
        path = self.paths.socket_file
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            if ipc_send("status", self.paths, timeout=0.25) is not None:
                raise RuntimeError("Padfire is already running.")
            path.unlink()
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            server.bind(str(path))
            os.chmod(path, 0o600)
            server.listen(8)
            server.settimeout(0.5)
        except Exception:
            server.close()
            path.unlink(missing_ok=True)
            raise
        self._server = server
        self._thread = threading.Thread(target=self._serve_unix, name="padfire-ipc", daemon=True)
        self._thread.start()

    def _serve_unix(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._server.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(target=self._handle_unix, args=(conn,), daemon=True).start()

    def _handle_unix(self, conn: socket.socket) -> None:
        try:
            conn.settimeout(1.0)
            data = bytearray()
            while len(data) < 4096 and b"\n" not in data:
                chunk = conn.recv(1024)
                if not chunk:
                    break
                data.extend(chunk)
            command = bytes(data).split(b"\n", 1)[0].decode("utf-8", errors="replace")
            conn.sendall(((self.handler(command) or "ok") + "\n").encode("utf-8"))
        except OSError:
            pass
        finally:
            conn.close()

    def _serve_windows(self) -> None:
        try:
            listener = mpconn.Listener(_pipe_name(self.paths.socket_file), family="AF_PIPE", authkey=_AUTHKEY)
            self._pipe_listener = listener
            self._ready.set()
            while not self._stop.is_set():
                conn = listener.accept()
                threading.Thread(target=self._handle_windows, args=(conn,), daemon=True).start()
        except Exception as exc:
            if not self._ready.is_set():
                self._startup_error = str(exc)
                self._ready.set()
            elif not self._stop.is_set():
                self._startup_error = str(exc)
        finally:
            if self._pipe_listener is not None:
                self._pipe_listener.close()
                self._pipe_listener = None

    def _handle_windows(self, conn: object) -> None:
        try:
            command = conn.recv_bytes(4096).decode("utf-8", errors="replace").strip()
            conn.send_bytes((self.handler(command) or "ok").encode("utf-8"))
        except (OSError, EOFError):
            pass
        finally:
            conn.close()

    def close(self) -> None:
        self._stop.set()
        if os.name == "nt":
            try:
                conn = mpconn.Client(_pipe_name(self.paths.socket_file), family="AF_PIPE", authkey=_AUTHKEY)
                conn.send_bytes(b"__wake__")
                try:
                    conn.recv_bytes(4096)
                except (OSError, EOFError):
                    pass
                conn.close()
            except (OSError, EOFError, mpconn.AuthenticationError):
                pass
            if self._pipe_listener is not None:
                self._pipe_listener.close()
        elif self._server is not None:
            self._server.close()
            self._server = None
        if self._thread is not None and self._thread is not threading.current_thread():
            self._thread.join(timeout=1.25)
        if os.name != "nt":
            self.paths.socket_file.unlink(missing_ok=True)
