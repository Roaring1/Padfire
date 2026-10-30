"""Qt-based Windows UI for Padfire; pad/config behavior matches the GTK edition."""
from __future__ import annotations

import copy
import ctypes
import json
import os
import shlex
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

from PySide6.QtCore import QObject, QPoint, QRunnable, QThreadPool, QTimer, Qt, Signal, Slot
from PySide6.QtGui import QAction, QDrag, QIcon, QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QApplication, QCheckBox, QComboBox, QDialog, QDialogButtonBox, QFileDialog,
    QFormLayout, QGridLayout, QGroupBox, QHBoxLayout, QLabel, QLineEdit, QMainWindow,
    QMenu, QMessageBox, QPlainTextEdit, QProgressBar, QPushButton, QSlider,
    QStackedWidget, QStyle, QSystemTrayIcon, QVBoxLayout, QWidget,
)

from padfire_core import (
    CC_TOP, CONFIG_VERSION,
    LED_AMBER, LED_DIM, LED_GREEN, LED_OFF, LED_ORANGE, LED_PG_HAS, LED_RED,
    NUM_COLS, NUM_PAGES, NUM_ROWS, VERSION, get_paths, load_config_file,
    migrate_legacy_config, save_config_file,
)
from .audio import AudioEngine
from .ipc import IPCServer, ipc_send
from .midi import MidiEngine, note_side, note_xy


PAD_COLORS: dict[str, tuple[int | None, str | None]] = {
    "default": (None, None),
    "green": (LED_GREEN, "#1d6d3b"),
    "orange": (LED_ORANGE, "#815018"),
    "red": (LED_RED, "#7d2929"),
    "amber": (LED_AMBER, "#80651b"),
    "dim green": (LED_DIM, "#19472c"),
}
ACTION_LABELS = [
    ("Play a sound", "play"),
    ("Run a script", "script"),
    ("Stop SFX", "stop_sfx"),
    ("Stop everything", "stop_all"),
    ("Stop a specific pad", "stop_key"),
]
AUDIO_FILTER = "Audio files (*.wav *.flac *.ogg *.mp3 *.aiff *.aif *.opus);;All files (*)"


_INSTANCE_MUTEX_HANDLE: int | None = None
ERROR_ALREADY_EXISTS = 183


def _acquire_windows_instance_mutex(server_name: str) -> tuple[int | None, bool]:
    """Use a per-user, per-session Windows mutex to enforce one GUI instance.

    QLocalServer uses named pipes on Windows, where Qt explicitly permits
    multiple servers to listen on the same pipe name. The mutex makes the
    single-instance guarantee independent of that pipe behavior.
    """
    if os.name != "nt":
        return None, False
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_bool, ctypes.c_wchar_p]
    kernel32.CreateMutexW.restype = ctypes.c_void_p
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel32.CloseHandle.restype = ctypes.c_bool
    ctypes.set_last_error(0)
    handle = kernel32.CreateMutexW(None, True, "Local\\" + server_name)
    last_error = ctypes.get_last_error()
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    if last_error == ERROR_ALREADY_EXISTS:
        kernel32.CloseHandle(handle)
        return None, True
    return int(handle), False


def _release_windows_instance_mutex(handle: int | None) -> None:
    if not handle or os.name != "nt":
        return
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.ReleaseMutex.argtypes = [ctypes.c_void_p]
    kernel32.ReleaseMutex.restype = ctypes.c_bool
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel32.CloseHandle.restype = ctypes.c_bool
    kernel32.ReleaseMutex(ctypes.c_void_p(handle))
    kernel32.CloseHandle(ctypes.c_void_p(handle))


def command_for_platform(command: str) -> tuple[str | list[str], bool]:
    """Run native script files without requiring a Unix shell on Windows.

    Arbitrary command strings retain the original shell behavior. A directly
    selected .ps1 or .py file is invoked with its interpreter explicitly.
    PowerShell's configured execution policy is respected (no implicit Bypass).
    """
    if os.name != "nt":
        return command, True
    try:
        parts = shlex.split(command, posix=False)
    except ValueError:
        return command, True
    if not parts:
        return command, True
    first = parts[0].strip('"')
    suffix = Path(first).suffix.casefold()
    if suffix == ".ps1":
        interpreter = shutil.which("pwsh.exe") or shutil.which("powershell.exe")
        if interpreter:
            return [interpreter, "-NoProfile", "-File", first, *parts[1:]], False
    if suffix == ".py" and Path(first).is_file():
        if getattr(sys, "frozen", False):
            # The frozen executable is Padfire.exe, not a Python interpreter.
            # A packaged install may still run user-authored Python pads when
            # a separate Python installation is available on PATH.
            interpreter = shutil.which("python.exe") or shutil.which("py.exe")
            if not interpreter:
                raise FileNotFoundError(
                    "Running .py pads from the packaged app requires python.exe or py.exe on PATH."
                )
        else:
            interpreter = sys.executable
        return [interpreter, first, *parts[1:]], False
    return command, True



class _Signals(QObject):
    pad = Signal(int, int, bool)
    top = Signal(int, bool)
    side = Signal(int, bool)
    midi_connected = Signal()
    midi_disconnected = Signal()
    ipc = Signal(str)


class _AnalysisSignals(QObject):
    finished = Signal(object)


class _AnalysisTask(QRunnable):
    def __init__(self, filename: str) -> None:
        super().__init__()
        self.filename = filename
        self.signals = _AnalysisSignals()

    @Slot()
    def run(self) -> None:
        self.signals.finished.emit(AudioEngine.analyze_loudness(self.filename))


class PadButton(QPushButton):
    """Pad cell supporting mouse activation, pad drag/copy, and file drops."""

    contextRequested = Signal(int, int, QPoint)
    fileDropped = Signal(int, int, str)
    padDropped = Signal(int, int, str, bool)

    def __init__(self, row: int, col: int, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.row = row
        self.col = col
        self._drag_origin: QPoint | None = None
        self.setAcceptDrops(True)
        self.setFixedSize(55 if col != 8 else 42, 55)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setToolTip("Click to play. Right-click to edit. Drag a file here to assign it.")

    def mousePressEvent(self, event: Any) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_origin = event.position().toPoint()
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event: Any) -> None:
        if (
            self._drag_origin is not None
            and event.buttons() & Qt.MouseButton.LeftButton
            and (event.position().toPoint() - self._drag_origin).manhattanLength()
            >= QApplication.startDragDistance()
            and self.property("pad-assigned")
        ):
            drag = QDrag(self)
            mime = self.mimeDataForPad()
            drag.setMimeData(mime)
            drag.exec(Qt.DropAction.MoveAction | Qt.DropAction.CopyAction)
            self._drag_origin = None
            return
        super().mouseMoveEvent(event)

    def mimeDataForPad(self) -> Any:
        from PySide6.QtCore import QMimeData
        mime = QMimeData()
        mime.setData("application/x-padfire-pad", f"{self.row},{self.col}".encode("ascii"))
        return mime

    def dragEnterEvent(self, event: Any) -> None:
        mime = event.mimeData()
        if mime.hasFormat("application/x-padfire-pad") or mime.hasUrls():
            event.acceptProposedAction()
        else:
            event.ignore()

    def dragMoveEvent(self, event: Any) -> None:
        if event.mimeData().hasFormat("application/x-padfire-pad") or event.mimeData().hasUrls():
            event.acceptProposedAction()
        else:
            event.ignore()

    def dropEvent(self, event: Any) -> None:
        mime = event.mimeData()
        if mime.hasFormat("application/x-padfire-pad"):
            try:
                source = bytes(mime.data("application/x-padfire-pad")).decode("ascii")
                self.padDropped.emit(
                    self.row, self.col, source,
                    bool(event.keyboardModifiers() & Qt.KeyboardModifier.ControlModifier),
                )
                event.setDropAction(
                    Qt.DropAction.CopyAction
                    if event.keyboardModifiers() & Qt.KeyboardModifier.ControlModifier
                    else Qt.DropAction.MoveAction
                )
                event.accept()
                return
            except (ValueError, UnicodeDecodeError):
                event.ignore()
                return
        if mime.hasUrls():
            local = next((url.toLocalFile() for url in mime.urls() if url.isLocalFile()), "")
            if local and Path(local).is_file():
                self.fileDropped.emit(self.row, self.col, local)
                event.acceptProposedAction()
                return
        event.ignore()

    def contextMenuEvent(self, event: Any) -> None:
        self.contextRequested.emit(self.row, self.col, event.globalPos())
        event.accept()


class PadEditor(QDialog):
    """Editor for one pad assignment, including all v5 actions and options."""

    def __init__(
        self,
        parent: QWidget,
        pad: dict[str, Any] | None,
        sinks: list[str],
        default_sink: str,
        stop_targets: list[tuple[str, str]],
        audio: AudioEngine,
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle("Edit Pad")
        self.setMinimumWidth(440)
        self.pad = copy.deepcopy(pad or {})
        self.audio = audio
        self._analysis_result: dict[str, Any] | None = None
        self._analysis_task: _AnalysisTask | None = None

        root = QVBoxLayout(self)
        form = QFormLayout()
        self.action = QComboBox()
        for label, value in ACTION_LABELS:
            self.action.addItem(label, value)
        current_action = self.pad.get("action", "play")
        index = self.action.findData(current_action)
        self.action.setCurrentIndex(index if index >= 0 else 0)
        form.addRow("Action", self.action)

        self.label = QLineEdit(str(self.pad.get("label", "")))
        self.label.setPlaceholderText("Name shown on the pad")
        form.addRow("Name", self.label)
        self.color = QComboBox()
        for name in PAD_COLORS:
            self.color.addItem(name.title() if name != "dim green" else "Dim green", name)
        color_index = self.color.findData(self.pad.get("color", "default"))
        self.color.setCurrentIndex(max(0, color_index))
        form.addRow("Pad color", self.color)
        self.notes = QPlainTextEdit(str(self.pad.get("notes", "")))
        self.notes.setMaximumHeight(68)
        form.addRow("Notes", self.notes)
        root.addLayout(form)

        self.stack = QStackedWidget()
        root.addWidget(self.stack)
        self._build_play_page(sinks, default_sink)
        self._build_script_page()
        self._build_stop_page(stop_targets)
        self.stack.addWidget(self._empty_action_page("This action does not need additional settings."))
        self.action.currentIndexChanged.connect(self._select_action)
        self._select_action(self.action.currentIndex())

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Cancel | QDialogButtonBox.StandardButton.Save
        )
        buttons.rejected.connect(self.reject)
        buttons.accepted.connect(self.accept)
        root.addWidget(buttons)

    def _empty_action_page(self, text: str) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.addWidget(QLabel(text))
        layout.addStretch(1)
        return page

    def _build_play_page(self, sinks: list[str], default_sink: str) -> None:
        page = QWidget()
        layout = QVBoxLayout(page)
        form = QFormLayout()
        file_row = QWidget()
        file_layout = QHBoxLayout(file_row)
        file_layout.setContentsMargins(0, 0, 0, 0)
        self.file = QLineEdit(str(self.pad.get("file", "")))
        self.file.setPlaceholderText("Choose an audio file")
        browse = QPushButton("Browse…")
        browse.clicked.connect(self._browse_audio)
        file_layout.addWidget(self.file, 1)
        file_layout.addWidget(browse)
        form.addRow("Sound file", file_row)

        self.output = QComboBox()
        self.output.addItem("System default", "")
        for sink in sinks:
            self.output.addItem(sink, sink)
        current = str(self.pad.get("sink", default_sink or ""))
        index = self.output.findData(current)
        if index < 0 and current:
            # Keep an old device name visible so the user can explicitly replace it.
            self.output.addItem(f"{current} (not present)", current)
            index = self.output.count() - 1
        self.output.setCurrentIndex(max(0, index))
        form.addRow("Output device", self.output)

        self.volume = QSlider(Qt.Orientation.Horizontal)
        self.volume.setRange(0, 150)
        self.volume.setValue(max(0, min(150, int(self.pad.get("vol", 80)))))
        self.volume_value = QLabel(f"{self.volume.value()}%")
        self.volume.valueChanged.connect(lambda n: self.volume_value.setText(f"{n}%"))
        volume_row = QWidget()
        volume_layout = QHBoxLayout(volume_row)
        volume_layout.setContentsMargins(0, 0, 0, 0)
        volume_layout.addWidget(self.volume, 1)
        volume_layout.addWidget(self.volume_value)
        form.addRow("Volume", volume_row)

        self.loop = QCheckBox("Repeat while active")
        self.loop.setChecked(bool(self.pad.get("loop", False)))
        form.addRow("Loop", self.loop)
        self.retrigger = QComboBox()
        for label, value in (
            ("Restart from start", "restart"),
            ("Stop on second press", "toggle"),
            ("Use global setting", "global"),
        ):
            self.retrigger.addItem(label, value)
        index = self.retrigger.findData(self.pad.get("retrigger", "global"))
        self.retrigger.setCurrentIndex(max(0, index))
        form.addRow("Second press", self.retrigger)
        layout.addLayout(form)

        loudness_row = QHBoxLayout()
        self.analyze = QPushButton("Check loudness")
        self.analyze_label = QLabel("")
        self.apply_loudness = QPushButton("Use")
        self.apply_loudness.setEnabled(False)
        self.analyze.clicked.connect(self._analyze)
        self.apply_loudness.clicked.connect(self._apply_loudness)
        loudness_row.addWidget(self.analyze)
        loudness_row.addWidget(self.analyze_label, 1)
        loudness_row.addWidget(self.apply_loudness)
        layout.addLayout(loudness_row)

        test_row = QHBoxLayout()
        test_play = QPushButton("Test play")
        test_stop = QPushButton("Stop test")
        test_play.clicked.connect(self._test_play)
        test_stop.clicked.connect(lambda: self.audio.stop("__editor_test__"))
        test_row.addWidget(test_play)
        test_row.addWidget(test_stop)
        test_row.addStretch(1)
        layout.addLayout(test_row)
        layout.addStretch(1)
        self.stack.addWidget(page)
        self.play_page = page

    def _build_script_page(self) -> None:
        page = QWidget()
        form = QFormLayout(page)
        cmd_row = QWidget()
        cmd_layout = QHBoxLayout(cmd_row)
        cmd_layout.setContentsMargins(0, 0, 0, 0)
        self.command = QLineEdit(str(self.pad.get("cmd", "")))
        self.command.setPlaceholderText("Command, .cmd, .bat or .ps1 path")
        browse = QPushButton("Browse…")
        browse.clicked.connect(self._browse_script)
        cmd_layout.addWidget(self.command, 1)
        cmd_layout.addWidget(browse)
        form.addRow("Command", cmd_row)
        self.pid_file = QLineEdit(str(self.pad.get("pid_file", "")))
        self.pid_file.setPlaceholderText("Optional PID file")
        form.addRow("PID file", self.pid_file)
        test = QPushButton("Test run")
        test.clicked.connect(self._test_script)
        form.addRow("", test)
        self.stack.addWidget(page)
        self.script_page = page

    def _build_stop_page(self, targets: list[tuple[str, str]]) -> None:
        page = QWidget()
        form = QFormLayout(page)
        self.stop_target = QComboBox()
        for key, label in targets:
            self.stop_target.addItem(label, key)
        current = self.pad.get("stop_key", "")
        index = self.stop_target.findData(current)
        if index >= 0:
            self.stop_target.setCurrentIndex(index)
        self.stop_target.setEnabled(bool(targets))
        if not targets:
            self.stop_target.addItem("(assign a sound or script pad first)", "")
        form.addRow("Target pad", self.stop_target)
        self.stack.addWidget(page)
        self.stop_page = page

    def _select_action(self, index: int) -> None:
        action = self.action.itemData(index)
        if action == "play":
            self.stack.setCurrentWidget(self.play_page)
        elif action == "script":
            self.stack.setCurrentWidget(self.script_page)
        elif action == "stop_key":
            self.stack.setCurrentWidget(self.stop_page)
        else:
            self.stack.setCurrentIndex(3)

    def _browse_audio(self) -> None:
        filename, _ = QFileDialog.getOpenFileName(self, "Choose audio file", self.file.text(), AUDIO_FILTER)
        if filename:
            self.file.setText(filename)
            if not self.label.text().strip():
                self.label.setText(Path(filename).stem)

    def _browse_script(self) -> None:
        filename, _ = QFileDialog.getOpenFileName(
            self, "Choose script", self.command.text(),
            "Scripts (*.ps1 *.cmd *.bat *.exe *.py);;All files (*)"
        )
        if filename:
            # Browse returns a bare path; keep spaces as part of the first
            # command token when the user later launches the selected script.
            self.command.setText(subprocess.list2cmdline([filename]))

    def _test_play(self) -> None:
        filename = self.file.text().strip()
        if filename and Path(filename).is_file():
            self.audio.play(
                "__editor_test__", filename, self.output.currentData(),
                self.volume.value(), self.loop.isChecked()
            )
        else:
            QMessageBox.warning(self, "Choose a file", "Select an existing sound file first.")

    def _analyze(self) -> None:
        filename = self.file.text().strip()
        if not filename or not Path(filename).is_file():
            QMessageBox.warning(self, "Choose a file", "Select an existing sound file first.")
            return
        self.analyze.setEnabled(False)
        self.analyze_label.setText("Analyzing…")
        task = _AnalysisTask(filename)
        task.signals.finished.connect(self._analysis_finished)
        self._analysis_task = task
        QThreadPool.globalInstance().start(task)

    @Slot(object)
    def _analysis_finished(self, result: object) -> None:
        self.analyze.setEnabled(True)
        if not isinstance(result, dict):
            self.analyze_label.setText("Could not analyze file")
            return
        if result.get("silent"):
            self.analyze_label.setText("File is silent")
            return
        self._analysis_result = result
        self.analyze_label.setText(
            f"{float(result['input_i']):.1f} LUFS · peak {float(result['input_tp']):.1f} dBFS "
            f"· suggested {int(result['suggested_vol'])}%"
        )
        self.apply_loudness.setEnabled(True)

    def _apply_loudness(self) -> None:
        if self._analysis_result:
            self.volume.setValue(int(self._analysis_result["suggested_vol"]))

    def _test_script(self) -> None:
        command = self.command.text().strip()
        if not command:
            return
        try:
            launch, use_shell = command_for_platform(command)
            subprocess.Popen(launch, shell=use_shell)
        except OSError as exc:
            QMessageBox.warning(self, "Script failed", str(exc))

    def assignment(self) -> dict[str, Any] | None:
        action = self.action.currentData()
        color = self.color.currentData() or "default"
        label = self.label.text().strip()
        notes = self.notes.toPlainText().strip()
        if action == "play":
            filename = self.file.text().strip()
            if not filename:
                QMessageBox.warning(self, "Missing sound", "Choose an audio file for this pad.")
                return None
            return {
                "action": "play", "file": filename,
                "sink": self.output.currentData() or "",
                "vol": self.volume.value(), "loop": self.loop.isChecked(),
                "label": label, "retrigger": self.retrigger.currentData(),
                "color": color, "notes": notes,
            }
        if action == "script":
            command = self.command.text().strip()
            if not command:
                QMessageBox.warning(self, "Missing command", "Enter a command or script path.")
                return None
            return {
                "action": "script", "cmd": command,
                "pid_file": self.pid_file.text().strip(),
                "label": label or Path(command.split()[0].strip('"')).stem,
                "color": color, "notes": notes,
            }
        if action == "stop_key":
            target = self.stop_target.currentData()
            if not target:
                QMessageBox.warning(self, "No target", "Assign a sound or script pad first.")
                return None
            return {
                "action": "stop_key", "stop_key": target,
                "label": label or "Stop", "color": color, "notes": notes,
            }
        default_label = {"stop_sfx": "Stop SFX", "stop_all": "Stop All"}.get(action, "Stop")
        return {"action": action, "label": label or default_label, "color": color, "notes": notes}

    def accept(self) -> None:
        result = self.assignment()
        if result is not None:
            super().accept()

    def result_config(self) -> dict[str, Any]:
        return self.assignment() or {}


class SettingsDialog(QDialog):
    def __init__(self, parent: QWidget, cfg: dict[str, Any], sinks: list[str]) -> None:
        super().__init__(parent)
        self.setWindowTitle("Padfire Settings")
        self.setMinimumWidth(390)
        form = QFormLayout(self)

        self.output = QComboBox()
        self.output.addItem("System default", "")
        for sink in sinks:
            self.output.addItem(sink, sink)
        index = self.output.findData(cfg.get("default_sink", ""))
        self.output.setCurrentIndex(max(0, index))
        form.addRow("Default output", self.output)

        self.stop_on_change = QCheckBox("Stop all sounds when changing page")
        self.stop_on_change.setChecked(bool(cfg.get("stop_on_page_change", False)))
        form.addRow("", self.stop_on_change)
        self.retrigger = QComboBox()
        self.retrigger.addItem("Restart from start", "restart")
        self.retrigger.addItem("Stop on second press", "toggle")
        index = self.retrigger.findData(cfg.get("retrigger", "restart"))
        self.retrigger.setCurrentIndex(max(0, index))
        form.addRow("Global second press", self.retrigger)

        self.monitor_enabled = QCheckBox("Also play sound to a second output")
        self.monitor_enabled.setChecked(bool(cfg.get("monitor_enabled", False)))
        form.addRow("", self.monitor_enabled)
        self.monitor_output = QComboBox()
        self.monitor_output.addItem("System default", "")
        for sink in sinks:
            self.monitor_output.addItem(sink, sink)
        index = self.monitor_output.findData(cfg.get("monitor_sink", ""))
        self.monitor_output.setCurrentIndex(max(0, index))
        form.addRow("Monitor output", self.monitor_output)
        hint = QLabel(
            "Windows uses the selected Windows audio endpoint. Existing Linux "
            "sink names are retained in the config but fall back to the system default "
            "until reassigned."
        )
        hint.setWordWrap(True)
        form.addRow("", hint)

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Cancel | QDialogButtonBox.StandardButton.Save
        )
        buttons.rejected.connect(self.reject)
        buttons.accepted.connect(self.accept)
        form.addRow(buttons)

    def result_config(self) -> dict[str, Any]:
        return {
            "default_sink": self.output.currentData() or "",
            "stop_on_page_change": self.stop_on_change.isChecked(),
            "retrigger": self.retrigger.currentData(),
            "monitor_enabled": self.monitor_enabled.isChecked(),
            "monitor_sink": self.monitor_output.currentData() or "",
        }


class PadfireWindow(QMainWindow):
    def __init__(self, *, enable_hardware: bool = True, start_ipc: bool = True) -> None:
        super().__init__()
        self.paths = get_paths()
        migrate_legacy_config(self.paths.config_file)
        self.cfg = load_config_file(self.paths.config_file, windows=os.name == "nt")
        # Create defaults on first launch, but preserve an unreadable existing file.
        if not self.paths.config_file.exists():
            save_config_file(self.cfg, self.paths.config_file)
        self._page = 0
        self._quitting = False
        self._signals = _Signals()
        self._signals.pad.connect(self._on_midi_pad)
        self._signals.top.connect(self._on_midi_top)
        self._signals.side.connect(self._on_midi_side)
        self._signals.midi_connected.connect(self._on_midi_connected)
        self._signals.midi_disconnected.connect(self._on_midi_disconnected)
        self._signals.ipc.connect(self._dispatch_ipc)
        self.audio = AudioEngine(disabled=not enable_hardware)
        self.midi: MidiEngine | None = None
        self.script_procs: dict[str, subprocess.Popen[str]] = {}
        self._script_lock = threading.RLock()
        self._pad_buttons: dict[tuple[int, int], PadButton] = {}
        self._page_buttons: list[QPushButton] = []
        self._last_leds: dict[str, int] = {}
        self._led_dirty = True
        self._indicator: QSystemTrayIcon | None = None
        self._ipc: IPCServer | None = None
        self.setWindowTitle(f"Padfire {VERSION}")
        self.setMinimumSize(540, 580)
        self._build_ui()
        self._setup_tray()
        if enable_hardware:
            self.midi = MidiEngine(
                lambda r, c, pressed: self._signals.pad.emit(r, c, pressed),
                lambda i, pressed: self._signals.top.emit(i, pressed),
                lambda r, pressed: self._signals.side.emit(r, pressed),
                lambda: self._signals.midi_connected.emit(),
                lambda: self._signals.midi_disconnected.emit(),
            )
        if start_ipc:
            self._ipc = IPCServer(self._ipc_handler, self.paths)
            self._ipc.start()
            self.paths.pid_file.parent.mkdir(parents=True, exist_ok=True)
            self.paths.pid_file.write_text(str(os.getpid()), encoding="ascii")
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._tick)
        self._timer.start(180)

    def _build_ui(self) -> None:
        self.setStyleSheet("""
            QMainWindow, QDialog, QWidget { background: #191a1d; color: #e8e8eb; }
            QPushButton {
                background: #27292e; border: 1px solid #3a3d44; border-radius: 6px;
                padding: 5px 9px; color: #e3e5e9;
            }
            QPushButton:hover { background: #343740; border-color: #555b66; }
            QPushButton:checked { background: #183a60; border-color: #3989d8; }
            QPushButton[pad="true"] {
                background: #24262b; border: 1px solid #3b3e45;
                padding: 2px; font-size: 10px; font-weight: 600;
            }
            QPushButton[pad="true"]:hover { background: #343740; }
            QLineEdit, QPlainTextEdit, QComboBox, QSpinBox {
                background: #22242a; border: 1px solid #41454e; border-radius: 4px;
                padding: 5px; selection-background-color: #1769aa;
            }
            QComboBox QAbstractItemView { background: #22242a; selection-background-color: #1769aa; }
            QProgressBar { border: 0; background: #303238; max-height: 6px; text-align: center; }
            QProgressBar::chunk { background: #36bd73; }
            QGroupBox { border: 1px solid #3a3d44; border-radius: 6px; margin-top: 10px; padding: 8px; }
            QGroupBox::title { subcontrol-origin: margin; left: 10px; padding: 0 4px; }
        """)
        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(12, 12, 12, 10)
        root.setSpacing(10)
        header = QHBoxLayout()
        title = QLabel("P A D F I R E")
        title.setStyleSheet("color:#3e9cf0;font-weight:900;letter-spacing:4px;font-size:15px;")
        header.addWidget(title)
        header.addStretch(1)
        self.midi_label = QLabel("○ MIDI")
        self.midi_label.setStyleSheet("color:#d07a70;font-weight:700;")
        header.addWidget(self.midi_label)
        self.stop_button = QPushButton("Stop")
        self.stop_button.clicked.connect(self.audio.stop_all)
        header.addWidget(self.stop_button)
        self.monitor_button = QPushButton("Mon")
        self.monitor_button.setCheckable(True)
        self.monitor_button.setChecked(bool(self.cfg.get("monitor_enabled", False)))
        self.monitor_button.clicked.connect(self._on_monitor_toggled)
        self.monitor_button.setToolTip("Duplicate playback to the configured monitor output")
        header.addWidget(self.monitor_button)
        settings = QPushButton("⚙")
        settings.setToolTip("Settings")
        settings.clicked.connect(self._open_settings)
        header.addWidget(settings)
        about = QPushButton("About")
        about.setObjectName("aboutButton")
        about.setToolTip("About Padfire and third-party licenses")
        about.clicked.connect(self._show_about)
        header.addWidget(about)
        root.addLayout(header)

        grid_wrap = QWidget()
        grid = QGridLayout(grid_wrap)
        grid.setContentsMargins(0, 0, 0, 0)
        grid.setSpacing(5)
        for page in range(NUM_PAGES):
            button = QPushButton(str(page + 1))
            button.setCheckable(True)
            button.setFixedSize(55, 28)
            button.setToolTip(f"Page {page + 1}")
            button.clicked.connect(lambda checked=False, value=page: self._switch_page(value))
            self._page_buttons.append(button)
            grid.addWidget(button, 0, page)
        spacer = QLabel("")
        spacer.setFixedWidth(42)
        grid.addWidget(spacer, 0, NUM_COLS)

        for row in range(NUM_ROWS):
            for col in range(NUM_COLS + 1):
                button = PadButton(row, col)
                button.setProperty("pad", True)
                button.clicked.connect(lambda checked=False, r=row, c=col: self._trigger_pad(r, c))
                button.contextRequested.connect(self._context_menu)
                button.fileDropped.connect(self._on_file_drop)
                button.padDropped.connect(self._on_pad_drop)
                grid.addWidget(button, row + 1, col)
                self._pad_buttons[(row, col)] = button
        grid_wrap.setFixedWidth(8 * 60 + 48)
        root.addWidget(grid_wrap, 0, Qt.AlignmentFlag.AlignHCenter)
        root.addStretch(1)

        status = QHBoxLayout()
        self.output_label = QLabel("OUT")
        self.output_label.setStyleSheet("color:#9ba0a8;font-size:10px;font-weight:700;")
        status.addWidget(self.output_label)
        self.vu_bar = QProgressBar()
        self.vu_bar.setRange(0, 100)
        self.vu_bar.setTextVisible(False)
        self.vu_bar.setFixedWidth(74)
        status.addWidget(self.vu_bar)
        self.hearth_label = QLabel("Hearth: Linux integration unavailable")
        self.hearth_label.setStyleSheet("color:#777d87;font-size:10px;")
        self.hearth_label.setToolTip("The existing Hearth IPC endpoint is Linux-specific.")
        status.addWidget(self.hearth_label)
        status.addStretch(1)
        self.page_status = QLabel("Page 1 / 8")
        self.page_status.setStyleSheet("color:#9ba0a8;font-size:10px;")
        status.addWidget(self.page_status)
        root.addLayout(status)
        self.setCentralWidget(central)

        for number in range(1, NUM_PAGES + 1):
            shortcut = QShortcut(QKeySequence(str(number)), self)
            shortcut.activated.connect(lambda value=number - 1: self._shortcut_page(value))
        stop_shortcut = QShortcut(QKeySequence("Space"), self)
        stop_shortcut.activated.connect(self._shortcut_stop)
        quit_shortcut = QShortcut(QKeySequence("Ctrl+Q"), self)
        quit_shortcut.activated.connect(self._quit)
        hide_shortcut = QShortcut(QKeySequence("Escape"), self)
        hide_shortcut.activated.connect(self._hide_or_minimize)
        self._refresh_all()

    def _shortcut_page(self, index: int) -> None:
        if isinstance(QApplication.focusWidget(), (QLineEdit, QPlainTextEdit)):
            return
        self._switch_page(index)

    def _shortcut_stop(self) -> None:
        if isinstance(QApplication.focusWidget(), (QLineEdit, QPlainTextEdit)):
            return
        self.audio.stop_all()

    def _setup_tray(self) -> None:
        if not QSystemTrayIcon.isSystemTrayAvailable():
            print("[tray] no Windows notification area; close exits the app")
            return
        self._indicator = QSystemTrayIcon(self)
        self._indicator.setIcon(QApplication.style().standardIcon(QStyle.StandardPixmap.SP_MediaPlay))
        self._indicator.setToolTip(f"Padfire — page {self._page + 1}")
        menu = QMenu()
        show_action = QAction("Show Padfire", self)
        show_action.triggered.connect(self._show)
        menu.addAction(show_action)
        stop_action = QAction("Stop all", self)
        stop_action.triggered.connect(self.audio.stop_all)
        menu.addAction(stop_action)
        settings_action = QAction("Settings", self)
        settings_action.triggered.connect(self._open_settings)
        menu.addAction(settings_action)
        about_action = QAction("About / licenses", self)
        about_action.triggered.connect(self._show_about)
        menu.addAction(about_action)
        menu.addSeparator()
        quit_action = QAction("Quit", self)
        quit_action.triggered.connect(self._quit)
        menu.addAction(quit_action)
        self._indicator.setContextMenu(menu)
        self._indicator.activated.connect(
            lambda reason: self._show()
            if reason in (QSystemTrayIcon.ActivationReason.Trigger,
                          QSystemTrayIcon.ActivationReason.DoubleClick) else None
        )
        self._indicator.show()
        self._update_tray()

    def _show_about(self) -> None:
        """Display the selected Qt/PySide LGPL path and bundled component licenses."""
        if getattr(sys, "frozen", False):
            app_dir = Path(sys.executable).resolve().parent
            license_dir = app_dir / "ThirdPartyLicenses"
            inventory = license_dir / "COMPONENTS.md"
        else:
            app_dir = Path(__file__).resolve().parents[1]
            license_dir = app_dir / "licenses"
            inventory = app_dir / "THIRD_PARTY_NOTICES.md"
        qt_lgpl_license = license_dir / "LGPL-3.0-only.txt"
        qt_gpl_license = license_dir / "GPL-3.0-only.txt"
        python_license = license_dir / "PSF-2.0.txt"
        QMessageBox.about(
            self,
            "About Padfire",
            "<h3>Padfire</h3>"
            f"<p>Version {VERSION}</p>"
            "<p>This application uses Qt 6, PySide6 and Shiboken6 under the "
            "<b>GNU Lesser General Public License version 3 (LGPLv3)</b>.</p>"
            f"<p>Qt/PySide LGPLv3 text: <code>{qt_lgpl_license}</code></p>"
            f"<p>GNU GPLv3 text: <code>{qt_gpl_license}</code></p>"
            f"<p>Component license inventory and notices: <code>{inventory}</code></p>"
            f"<p>Python runtime license: <code>{python_license}</code></p>"
            f"<p>License folder: <code>{license_dir}</code></p>"
            f"<p>Qt LGPLv3 file present: {qt_lgpl_license.is_file()}<br>"
            f"GNU GPLv3 file present: {qt_gpl_license.is_file()}<br>"
            f"Python license file present: {python_license.is_file()}<br>"
            f"Component inventory present: {inventory.is_file()}</p>"
        )

    def _persist(self) -> None:
        self.cfg["version"] = CONFIG_VERSION
        save_config_file(self.cfg, self.paths.config_file)

    def _pad_note(self, row: int, col: int) -> int:
        return note_side(row) if col == 8 else note_xy(row, col)

    def _pad_key(self, row: int, col: int) -> str:
        return f"{self._page}:{self._pad_note(row, col)}"

    def _pad(self, row: int, col: int) -> dict[str, Any] | None:
        page = self.cfg["pages"][self._page]
        value = page.get(str(self._pad_note(row, col)))
        return value if isinstance(value, dict) else None

    def _set_pad(self, row: int, col: int, value: dict[str, Any] | None) -> None:
        page = self.cfg["pages"][self._page]
        key = str(self._pad_note(row, col))
        if value is None:
            page.pop(key, None)
        else:
            page[key] = value
        self._persist()
        self._led_dirty = True
        self._refresh_all()

    def _all_pad_keys(self) -> list[tuple[str, str]]:
        output: list[tuple[str, str]] = []
        page = self.cfg["pages"][self._page]
        for note, pad in sorted(page.items(), key=lambda pair: int(pair[0])):
            if not isinstance(pad, dict) or pad.get("action") not in ("play", "script"):
                continue
            label = str(pad.get("label") or pad.get("file") or pad.get("cmd") or "Pad")
            output.append((f"{self._page}:{note}", f"{self._page + 1}:{note}  ({label})"))
        return output or [("", "(none)")]

    def _edit_pad(self, row: int, col: int, initial: dict[str, Any] | None = None) -> None:
        existing = copy.deepcopy(initial if initial is not None else self._pad(row, col))
        targets = self._all_pad_keys()
        dialog = PadEditor(
            self, existing, self.audio.list_sinks(refresh=True),
            str(self.cfg.get("default_sink") or ""),
            targets if targets != [("", "(none)")] else [],
            self.audio,
        )
        accepted = dialog.exec() == QDialog.DialogCode.Accepted
        self.audio.stop("__editor_test__")
        if accepted:
            self._set_pad(row, col, dialog.result_config())

    @Slot(int, int, QPoint)
    def _context_menu(self, row: int, col: int, global_pos: QPoint) -> None:
        menu = QMenu(self)
        edit = menu.addAction("Edit pad…" if self._pad(row, col) else "Assign pad…")
        clear = menu.addAction("Clear pad")
        clear.setEnabled(self._pad(row, col) is not None)
        selected = menu.exec(global_pos)
        if selected is edit:
            self._edit_pad(row, col)
        elif selected is clear:
            self._stop_key(self._pad_key(row, col))
            self._set_pad(row, col, None)

    @Slot(int, int, str)
    def _on_file_drop(self, row: int, col: int, filename: str) -> None:
        current = copy.deepcopy(self._pad(row, col) or {})
        current["file"] = filename
        current.setdefault("action", "play")
        if not current.get("label"):
            current["label"] = Path(filename).stem
        self._edit_pad(row, col, current)

    @Slot(int, int, str, bool)
    def _on_pad_drop(self, row: int, col: int, source: str, copy_pad: bool) -> None:
        try:
            src_row, src_col = (int(part) for part in source.split(",", 1))
        except (TypeError, ValueError):
            return
        if (src_row, src_col) == (row, col):
            return
        source_pad = self._pad(src_row, src_col)
        if source_pad is None:
            return
        self._set_pad(row, col, copy.deepcopy(source_pad))
        if not copy_pad:
            self._set_pad(src_row, src_col, None)

    def _trigger_pad(self, row: int, col: int) -> None:
        self._flash(row, col)
        pad = self._pad(row, col)
        if not pad:
            self._edit_pad(row, col)
            return
        action = pad.get("action", "play")
        key = self._pad_key(row, col)
        if action == "stop_sfx":
            self.audio.stop_all()
            return
        if action == "stop_all":
            self.audio.stop_all()
            self._stop_all_scripts()
            return
        if action == "stop_key":
            target = str(pad.get("stop_key", "")).split("  (", 1)[0]
            if target:
                self._stop_key(target)
            return
        if action == "script":
            self._run_script(key, pad)
            return
        if action != "play":
            return
        filename = str(pad.get("file", ""))
        if not filename or not Path(filename).is_file():
            self.statusBar().showMessage(f"Missing sound file: {Path(filename).name}", 5000)
            return
        retrigger = pad.get("retrigger", "global")
        if retrigger == "global":
            retrigger = self.cfg.get("retrigger", "restart")
        if self.audio.is_playing(key) and retrigger == "toggle":
            self.audio.stop(key)
            return
        monitor = (
            str(self.cfg.get("monitor_sink") or "")
            if self.cfg.get("monitor_enabled") else None
        )
        self.audio.play(
            key, filename,
            str(pad.get("sink", self.cfg.get("default_sink", "")) or ""),
            int(pad.get("vol", 80)),
            bool(pad.get("loop", False)),
            monitor_sink=monitor,
        )
        self._led_dirty = True

    def _run_script(self, key: str, pad: dict[str, Any]) -> None:
        command = str(pad.get("cmd", "")).strip()
        if not command:
            return
        with self._script_lock:
            old = self.script_procs.get(key)
            if old is not None and old.poll() is None:
                return

        def worker() -> None:
            process: subprocess.Popen[str] | None = None
            try:
                launch, use_shell = command_for_platform(command)
                process = subprocess.Popen(
                    launch, shell=use_shell, stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT, text=True, bufsize=1,
                    creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
                )
                with self._script_lock:
                    self.script_procs[key] = process
                output, _ = process.communicate(timeout=600)
                if output:
                    for line in output.strip().splitlines()[:10]:
                        print(f"[script] {line}")
            except subprocess.TimeoutExpired:
                print(f"[script] {key}: command exceeded the 10-minute limit")
                if process is not None:
                    process.kill()
            except OSError as exc:
                print(f"[script] {key}: {exc}")
            finally:
                with self._script_lock:
                    if self.script_procs.get(key) is process:
                        self.script_procs.pop(key, None)
                self._led_dirty = True

        threading.Thread(target=worker, name=f"padfire-script-{key}", daemon=True).start()
        self._led_dirty = True

    def _script_on(self, key: str, pad: dict[str, Any]) -> bool:
        with self._script_lock:
            process = self.script_procs.get(key)
        if process is not None:
            if process.poll() is None:
                return True
            with self._script_lock:
                self.script_procs.pop(key, None)
        pid_file = str(pad.get("pid_file", "")).strip()
        if not pid_file:
            return False
        try:
            for value in Path(pid_file).read_text(encoding="ascii").split():
                try:
                    pid = int(value)
                    if os.name == "nt":
                        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
                        kernel32.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_bool, ctypes.c_ulong]
                        kernel32.OpenProcess.restype = ctypes.c_void_p
                        kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
                        kernel32.CloseHandle.restype = ctypes.c_bool
                        handle = kernel32.OpenProcess(0x1000, False, pid)
                        if handle:
                            kernel32.CloseHandle(handle)
                            return True
                    else:
                        os.kill(pid, 0)
                        return True
                except (ProcessLookupError, PermissionError, ValueError, OSError):
                    continue
        except OSError:
            pass
        return False

    def _stop_key(self, key: str) -> None:
        # Accept either the current page key or a page:note key chosen in the editor.
        self.audio.stop(key)
        with self._script_lock:
            process = self.script_procs.pop(key, None)
        if process is not None and process.poll() is None:
            if os.name == "nt":
                # Popen(shell=True) owns cmd.exe; terminate the process tree so
                # the script it launched cannot remain orphaned after Stop.
                try:
                    subprocess.run(
                        ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                        capture_output=True, text=True, timeout=2.0, check=False,
                    )
                    process.wait(timeout=1.0)
                except (OSError, subprocess.TimeoutExpired):
                    try:
                        process.kill()
                    except OSError:
                        pass
            else:
                try:
                    process.terminate()
                    process.wait(timeout=1.5)
                except Exception:
                    try:
                        process.kill()
                    except OSError:
                        pass
        self._led_dirty = True

    def _stop_all_scripts(self) -> None:
        with self._script_lock:
            keys = list(self.script_procs)
        for key in keys:
            self._stop_key(key)

    def _switch_page(self, index: int) -> None:
        index = max(0, min(NUM_PAGES - 1, int(index)))
        if index == self._page:
            return
        if self.cfg.get("stop_on_page_change"):
            self.audio.stop_all()
        self._page = index
        self._last_leds.clear()
        self._led_dirty = True
        self._refresh_all()
        self._update_tray()

    def _flash(self, row: int, col: int) -> None:
        button = self._pad_buttons.get((row, col))
        if button is None:
            return
        button.setProperty("flash", True)
        self._render_pad(button, self._pad(row, col), self._pad_key(row, col))
        QTimer.singleShot(100, lambda: self._unflash(row, col))

    def _unflash(self, row: int, col: int) -> None:
        button = self._pad_buttons.get((row, col))
        if button is not None:
            button.setProperty("flash", False)
            self._render_pad(button, self._pad(row, col), self._pad_key(row, col))

    def _led_for(self, page_index: int, page: dict[str, Any], note: int) -> int:
        key = f"{page_index}:{note}"
        pad = page.get(str(note))
        if not isinstance(pad, dict):
            return LED_OFF
        action = pad.get("action", "play")
        color = PAD_COLORS.get(pad.get("color", "default"), PAD_COLORS["default"])[0]
        if action in ("stop_sfx", "stop_all", "stop_key"):
            return color if color is not None else LED_RED
        if action == "script":
            return LED_GREEN if self._script_on(key, pad) else (color or LED_ORANGE)
        if self.audio.is_playing(key):
            return LED_GREEN
        return color if color is not None else LED_DIM

    def _refresh_leds(self) -> None:
        if not self.midi or not self.midi.is_connected() or not self._led_dirty:
            return
        page = self.cfg["pages"][self._page]
        wanted: dict[str, tuple[int, int]] = {}
        for row in range(NUM_ROWS):
            for col in range(NUM_COLS):
                note = note_xy(row, col)
                wanted[f"n:{note}"] = (note, self._led_for(self._page, page, note))
            note = note_side(row)
            wanted[f"n:{note}"] = (note, self._led_for(self._page, page, note))
        for index, control in enumerate(CC_TOP):
            color = LED_AMBER if index == self._page else (
                LED_PG_HAS if self.cfg["pages"][index] else LED_OFF
            )
            wanted[f"c:{control}"] = (control, color)
        for key, (number, color) in wanted.items():
            if self._last_leds.get(key) == color:
                continue
            if key.startswith("n:"):
                self.midi.led(number, color)
            else:
                self.midi.cc_led(number, color)
            self._last_leds[key] = color
        self._led_dirty = False

    def _refresh_all(self) -> None:
        for (row, col), button in self._pad_buttons.items():
            pad = self._pad(row, col)
            self._render_pad(button, pad, self._pad_key(row, col))
        for index, button in enumerate(self._page_buttons):
            button.setChecked(index == self._page)
        self.page_status.setText(f"Page {self._page + 1} / {NUM_PAGES}")
        self._led_dirty = True
        self._refresh_leds()

    def _render_pad(
        self, button: PadButton, pad: dict[str, Any] | None, key: str
    ) -> None:
        if not pad:
            text = ""
            style = ""
            tooltip = "Empty pad — click to assign, or drag an audio file here."
        else:
            action = pad.get("action", "play")
            label = str(pad.get("label") or Path(str(pad.get("file") or pad.get("cmd") or "Pad")).stem)
            exists = action != "play" or (
                bool(pad.get("file")) and Path(str(pad.get("file"))).is_file()
            )
            missing = action == "play" and not exists
            playing = self.audio.is_playing(key)
            script_on = action == "script" and self._script_on(key, pad)
            prefix = "! " if missing else ("▶ " if playing or script_on else "")
            text = prefix + label
            base = PAD_COLORS.get(pad.get("color", "default"), PAD_COLORS["default"])[1]
            border = "#ff554f" if missing else ("#38d177" if playing or script_on else "#444850")
            style = f"QPushButton {{ border: {'2px dashed' if missing else '2px solid'} {border};"
            if base:
                style += f" background: {base};"
            if action in ("stop_sfx", "stop_all", "stop_key"):
                style += " color: #ff9c96;"
            elif action == "script":
                style += " color: #ffc875;"
            if button.property("flash"):
                style += " background: #555b66;"
            style += " }"
            action_label = next((label for label, code in ACTION_LABELS if code == action), str(action))
            details = [action_label]
            if pad.get("file"):
                details.append(str(pad["file"]))
            if pad.get("notes"):
                details.append(str(pad["notes"]))
            if missing:
                details.append("Sound file is missing — edit this pad to relink it.")
            tooltip = "\n".join(details)

        signature = (text, style, tooltip)
        if getattr(button, "_render_signature", None) == signature:
            return
        button._render_signature = signature
        button.setText(text)
        button.setProperty("pad-assigned", bool(pad))
        button.setStyleSheet(style)
        button.setToolTip(tooltip)

    def _tick(self) -> None:
        self.midi_label.setText("● MIDI" if self.midi and self.midi.is_connected() else "○ MIDI")
        self.midi_label.setStyleSheet(
            "color:#48d27d;font-weight:700;" if self.midi and self.midi.is_connected()
            else "color:#d07a70;font-weight:700;"
        )
        self.vu_bar.setValue(int(self.audio.level() * 100))
        for (row, col), button in self._pad_buttons.items():
            pad = self._pad(row, col)
            self._render_pad(button, pad, self._pad_key(row, col))
        # Reconcile hardware LEDs every UI tick. Audio may end naturally or be
        # stopped by the global Stop button without a pad action callback.
        self._led_dirty = True
        self._refresh_leds()

    def _on_monitor_toggled(self, checked: bool) -> None:
        self.cfg["monitor_enabled"] = checked
        self._persist()
        self.monitor_button.setText("Mon ●" if checked else "Mon")
        self._led_dirty = True

    def _open_settings(self) -> None:
        dialog = SettingsDialog(self, self.cfg, self.audio.list_sinks(refresh=True))
        if dialog.exec() == QDialog.DialogCode.Accepted:
            self.cfg.update(dialog.result_config())
            self._persist()
            self.monitor_button.setChecked(bool(self.cfg.get("monitor_enabled")))
            self.monitor_button.setText("Mon ●" if self.cfg.get("monitor_enabled") else "Mon")
            self._refresh_all()

    def _on_midi_pad(self, row: int, col: int, pressed: bool) -> None:
        if pressed:
            self._trigger_pad(row, col)
        else:
            self._unflash(row, col)

    def _on_midi_top(self, index: int, pressed: bool) -> None:
        if pressed:
            self._switch_page(index)

    def _on_midi_side(self, row: int, pressed: bool) -> None:
        if pressed:
            self._trigger_pad(row, 8)
        else:
            self._unflash(row, 8)

    def _on_midi_connected(self) -> None:
        self._last_leds.clear()
        self._led_dirty = True
        QTimer.singleShot(0, self._refresh_all)

    def _on_midi_disconnected(self) -> None:
        QTimer.singleShot(0, self._tick)

    def _update_tray(self) -> None:
        if self._indicator is not None:
            self._indicator.setToolTip(f"Padfire — page {self._page + 1}")

    def _show(self) -> None:
        self.showNormal()
        self.raise_()
        self.activateWindow()

    def _hide_or_minimize(self) -> None:
        if self._indicator is not None:
            self.hide()
        else:
            self.showMinimized()

    def closeEvent(self, event: Any) -> None:
        if not self._quitting and self._indicator is not None:
            self.hide()
            event.accept()
            return
        self._quit()
        event.accept()

    def _status(self) -> dict[str, Any]:
        return {
            "running": True,
            "version": VERSION,
            "page": self._page,
            "playing": self.audio.active_keys(),
            "midi_connected": bool(self.midi and self.midi.is_connected()),
            "platform": "windows" if os.name == "nt" else sys.platform,
        }

    def _ipc_handler(self, command: str) -> str:
        if command == "status":
            return json.dumps(self._status(), separators=(",", ":"))
        self._signals.ipc.emit(command)
        return "ok"

    @Slot(str)
    def _dispatch_ipc(self, command: str) -> None:
        if command == "show":
            self._show()
        elif command in ("stop", "stop_all"):
            self.audio.stop_all()
            self._stop_all_scripts()
        elif command == "reload":
            self.cfg = load_config_file(self.paths.config_file, windows=os.name == "nt")
            self._refresh_all()
        elif command == "quit":
            self._quit()

    def _quit(self) -> None:
        if self._quitting:
            return
        self._quitting = True
        if self.midi is not None:
            self.midi.shutdown()
        self.audio.stop_all()
        self._stop_all_scripts()
        if self._ipc is not None:
            self._ipc.close()
            self._ipc = None
        try:
            self.paths.pid_file.unlink(missing_ok=True)
        except OSError:
            pass
        if self._indicator is not None:
            self._indicator.hide()
        QApplication.instance().quit()


def install_startup(enable: bool) -> int:
    if os.name != "nt":
        print("Windows startup registration is available only on Windows.", file=sys.stderr)
        return 1
    import winreg

    if getattr(sys, "frozen", False):
        executable = Path(sys.executable).resolve()
        if executable.name.casefold() == "padfirecli.exe":
            gui_executable = executable.parent.parent / "Padfire.exe"
            if gui_executable.is_file():
                executable = gui_executable
        command = f'"{executable}"'
    else:
        script = Path(__file__).resolve().parents[1] / "padfire_windows.py"
        command = f'"{Path(sys.executable).resolve()}" "{script}"'
    key_path = r"Software\Microsoft\Windows\CurrentVersion\Run"
    try:
        with winreg.OpenKey(
            winreg.HKEY_CURRENT_USER, key_path, 0, winreg.KEY_SET_VALUE
        ) as key:
            if enable:
                winreg.SetValueEx(key, "Padfire", 0, winreg.REG_SZ, command)
                print("Padfire will start when this Windows user signs in.")
            else:
                try:
                    winreg.DeleteValue(key, "Padfire")
                except FileNotFoundError:
                    pass
                print("Padfire startup entry removed.")
        return 0
    except OSError as exc:
        print(f"Could not update Windows startup: {exc}", file=sys.stderr)
        return 1


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=f"Padfire {VERSION} (Windows)")
    parser.add_argument("--version", action="version", version=f"Padfire {VERSION}")
    parser.add_argument("--show", action="store_true", help="show the running Padfire window")
    parser.add_argument("--stop", action="store_true", help="stop all sounds and scripts")
    parser.add_argument("--reload", action="store_true", help="reload padfire.json")
    parser.add_argument("--quit", action="store_true", help="quit the running instance")
    parser.add_argument("--status", action="store_true", help="print machine-readable JSON status")
    parser.add_argument("--install", action="store_true", help="start Padfire at Windows sign-in")
    parser.add_argument("--uninstall", action="store_true", help="remove the Windows startup entry")
    parser.add_argument("--smoke-ui", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)

    if args.install:
        return install_startup(True)
    if args.uninstall:
        return install_startup(False)

    paths = get_paths()
    for flag, command in (
        ("quit", "quit"), ("show", "show"), ("reload", "reload"), ("stop", "stop_all")
    ):
        if getattr(args, flag):
            reply = ipc_send(command, paths)
            print(reply or "not running")
            return 0 if reply else 1

    if args.status:
        reply = ipc_send("status", paths)
        print(reply or json.dumps({"running": False, "version": VERSION}))
        return 0

    global _INSTANCE_MUTEX_HANDLE
    if os.name == "nt" and not args.smoke_ui:
        _INSTANCE_MUTEX_HANDLE, already_running = _acquire_windows_instance_mutex(
            str(paths.socket_file)
        )
        if already_running:
            # The mutex is created just before the server is started. Give a
            # first-launching instance a short window to begin listening.
            for _attempt in range(12):
                if ipc_send("show", paths, timeout=0.25) is not None:
                    break
                time.sleep(0.1)
            return 0

    # Also handles a previous version that may not own the new mutex yet.
    if ipc_send("status", paths):
        ipc_send("show", paths)
        return 0

    app = QApplication([sys.argv[0]])
    app.setApplicationName("Padfire")
    app.setOrganizationName("Roaring1")
    app.setQuitOnLastWindowClosed(False)
    app.aboutToQuit.connect(lambda: _release_windows_instance_mutex(_INSTANCE_MUTEX_HANDLE))
    window = PadfireWindow(enable_hardware=not args.smoke_ui, start_ipc=not args.smoke_ui)
    window.show()
    if args.smoke_ui:
        def verify() -> None:
            assert len(window._pad_buttons) == NUM_ROWS * (NUM_COLS + 1)
            assert len(window._page_buttons) == NUM_PAGES
            assert window.cfg["version"] == CONFIG_VERSION
            print("PASS: Qt UI created with 72 pads and eight pages.")
            window._quit()
        QTimer.singleShot(150, verify)
    return app.exec()
