"""Platform-native local IPC for Padfire.

Linux retains the historic AF_UNIX socket for Hearth compatibility. Windows
uses Qt's QLocalServer/QLocalSocket, which maps to named pipes rather than
pretending a Unix socket path is a Windows filesystem object.
"""
from __future__ import annotations

import os
import socket
import threading
from pathlib import Path
from typing import Callable

from padfire_core import PadfirePaths, get_paths


def ipc_send(
    command: str,
    paths: PadfirePaths | None = None,
    timeout: float = 0.75,
) -> str | None:
    target = paths or get_paths()
    if os.name == "nt":
        from PySide6.QtNetwork import QLocalSocket

        client = QLocalSocket()
        client.connectToServer(str(target.socket_file))
        if not client.waitForConnected(max(1, int(timeout * 1000))):
            client.abort()
            return None
        try:
            payload = (command.strip() + "\n").encode("utf-8")
            if client.write(payload) < 0 or not client.waitForBytesWritten(500):
                return None
            response = bytearray()
            deadline_ms = max(1, int(timeout * 1000))
            while len(response) < 65536 and b"\n" not in response:
                if not client.bytesAvailable() and not client.waitForReadyRead(deadline_ms):
                    break
                response.extend(bytes(client.readAll()))
            if not response:
                return None
            return bytes(response).split(b"\n", 1)[0].decode("utf-8", errors="replace")
        finally:
            client.disconnectFromServer()
            if client.state() != QLocalSocket.LocalSocketState.UnconnectedState:
                client.waitForDisconnected(250)

    path = str(target.socket_file)
    if not hasattr(socket, "AF_UNIX"):
        return None
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        client.settimeout(timeout)
        client.connect(path)
        client.sendall((command.strip() + "\n").encode("utf-8"))
        chunks: list[bytes] = []
        total = 0
        while total < 65536:
            chunk = client.recv(min(4096, 65536 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if b"\n" in chunk:
                break
        return b"".join(chunks).split(b"\n", 1)[0].decode("utf-8", errors="replace")
    except (OSError, TimeoutError):
        return None
    finally:
        client.close()


class IPCServer:
    def __init__(
        self,
        handler: Callable[[str], str | None],
        paths: PadfirePaths | None = None,
    ) -> None:
        self.paths = paths or get_paths()
        self.handler = handler
        self._server: socket.socket | None = None
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._ready = threading.Event()
        self._startup_error: str | None = None

    def start(self) -> None:
        if os.name == "nt":
            self._thread = threading.Thread(
                target=self._serve_windows,
                name="padfire-ipc",
                daemon=True,
            )
            self._thread.start()
            if not self._ready.wait(3.0):
                raise RuntimeError("Timed out while starting the Padfire Windows IPC pipe.")
            if self._startup_error:
                raise RuntimeError(self._startup_error)
            return

        path = self.paths.socket_file
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            if ipc_send("status", self.paths, timeout=0.25) is not None:
                raise RuntimeError("Padfire is already running (IPC socket is active).")
            try:
                path.unlink()
            except OSError as exc:
                raise RuntimeError(f"Cannot remove stale Padfire socket {path}: {exc}") from exc
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            server.bind(str(path))
            os.chmod(path, 0o600)
            server.listen(8)
            server.settimeout(0.5)
        except Exception:
            server.close()
            try:
                path.unlink(missing_ok=True)
            except OSError:
                pass
            raise
        self._server = server
        self._thread = threading.Thread(
            target=self._serve_unix, name="padfire-ipc", daemon=True
        )
        self._thread.start()

    def _serve_unix(self) -> None:
        assert self._server is not None
        while not self._stop.is_set():
            try:
                conn, _ = self._server.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(
                target=self._handle_unix,
                args=(conn,),
                name="padfire-ipc-client",
                daemon=True,
            ).start()

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
            response = self.handler(command) or "ok"
            conn.sendall((response + "\n").encode("utf-8"))
        except OSError:
            pass
        finally:
            conn.close()

    def _serve_windows(self) -> None:
        from PySide6.QtNetwork import QLocalServer, QLocalSocket

        server = QLocalServer()
        # Restrict the named pipe to the current Windows user.
        server.setSocketOptions(QLocalServer.SocketOption.UserAccessOption)
        if not server.listen(str(self.paths.socket_file)):
            self._startup_error = f"Cannot listen on Padfire IPC pipe: {server.errorString()}"
            self._ready.set()
            return
        self._ready.set()
        try:
            while not self._stop.is_set():
                if not server.waitForNewConnection(400):
                    continue
                client = server.nextPendingConnection()
                if client is None:
                    continue
                self._handle_windows(client)
        finally:
            server.close()

    def _handle_windows(self, client: object) -> None:
        from PySide6.QtNetwork import QLocalSocket

        try:
            sock = client
            sock.setReadBufferSize(4096)
            data = bytearray()
            while len(data) < 4096 and b"\n" not in data:
                if not sock.bytesAvailable() and not sock.waitForReadyRead(1000):
                    break
                data.extend(bytes(sock.readAll()))
            command = bytes(data).split(b"\n", 1)[0].decode("utf-8", errors="replace")
            response = self.handler(command) or "ok"
            sock.write((response + "\n").encode("utf-8"))
            sock.waitForBytesWritten(500)
        except (OSError, RuntimeError):
            pass
        finally:
            try:
                sock = client
                sock.disconnectFromServer()
                if sock.state() != QLocalSocket.LocalSocketState.UnconnectedState:
                    sock.waitForDisconnected(250)
                sock.deleteLater()
            except (OSError, RuntimeError):
                pass

    def close(self) -> None:
        self._stop.set()
        if self._server is not None:
            try:
                self._server.close()
            except OSError:
                pass
            self._server = None
        if self._thread is not None and self._thread is not threading.current_thread():
            self._thread.join(timeout=1.25)
        if os.name != "nt":
            try:
                self.paths.socket_file.unlink(missing_ok=True)
            except OSError:
                pass
