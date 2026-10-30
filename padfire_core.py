"""Shared, dependency-light Padfire configuration and platform paths.

This module is deliberately free of GTK, Qt, MIDI and audio imports so the
Linux and Windows front ends can share one on-disk configuration contract.
"""
from __future__ import annotations

import getpass
import hashlib
import json
import os
import shutil
import tempfile
from pathlib import Path
from typing import Any

VERSION = "2.4-dev"
LINUX_VERSION = "2.3"
CONFIG_VERSION = 5
NUM_PAGES = 8
NUM_ROWS = 8
NUM_COLS = 8

LED_OFF = 0x0C
LED_DIM = 0x1C
LED_GREEN = 0x3C
LED_RED = 0x0F
LED_ORANGE = 0x1F
LED_AMBER = 0x3F
LED_PG_HAS = 0x0D

DEVICE_SUBSTR = "Launchpad Mini"
DEVICE_EXCLUDE = ("MK3", "mk3", "Mk3")
CC_TOP = list(range(104, 112))


def default_config(*, windows: bool | None = None) -> dict[str, Any]:
    """Return a fresh v5 config with a sensible default for the current OS."""
    is_windows = os.name == "nt" if windows is None else windows
    return {
        "version": CONFIG_VERSION,
        "default_sink": "" if is_windows else "vm_game",
        "stop_on_page_change": False,
        "retrigger": "restart",
        "monitor_enabled": False,
        "monitor_sink": "" if is_windows else "alsa_output.usb-Astro_Gaming_Astro_A50-00.stereo-game",
        "pages": [{} for _ in range(NUM_PAGES)],
    }


# Kept as a module constant for the Linux UI and existing integrations.
DEFAULT_CONFIG = default_config(windows=False)


class PadfirePaths:
    """Resolved user-specific locations used by the application."""

    def __init__(
        self,
        config_dir: Path,
        config_file: Path,
        socket_file: Path,
        pid_file: Path,
        hearth_socket: Path | None,
    ) -> None:
        self.config_dir = config_dir
        self.config_file = config_file
        self.socket_file = socket_file
        self.pid_file = pid_file
        self.hearth_socket = hearth_socket


def get_paths(*, windows: bool | None = None) -> PadfirePaths:
    """Resolve config/runtime paths without assuming HOME or a shell wrapper.

    Linux keeps the established ~/.config/padfire layout (respecting
    XDG_CONFIG_HOME). Windows stores settings in %APPDATA% and puts the short
    AF_UNIX socket pathname in the temp directory to stay below Winsock's
    pathname limit.
    """
    is_windows = os.name == "nt" if windows is None else windows
    home = Path.home()
    if is_windows:
        appdata = Path(os.environ.get("APPDATA") or home / "AppData/Roaming")
        config_dir = appdata / "Padfire"
        identity = getpass.getuser().casefold().encode("utf-8", errors="replace")
        suffix = hashlib.sha256(identity).hexdigest()[:12]
        # QLocalServer maps this short logical name to a Windows named pipe.
        # It is not a filesystem path on Windows.
        runtime_socket = Path(f"Padfire-{suffix}")
        local = Path(os.environ.get("LOCALAPPDATA") or home / "AppData/Local")
        pid_file = local / "Padfire" / "padfire.pid"
        return PadfirePaths(
            config_dir=config_dir,
            config_file=config_dir / "padfire.json",
            socket_file=runtime_socket,
            pid_file=pid_file,
            hearth_socket=None,
        )

    raw_xdg = os.environ.get("XDG_CONFIG_HOME", "")
    xdg = Path(raw_xdg) if raw_xdg and Path(raw_xdg).is_absolute() else home / ".config"
    config_dir = xdg / "padfire"
    hearth_socket = xdg / "hearth" / "rac.sock"
    return PadfirePaths(
        config_dir=config_dir,
        config_file=config_dir / "padfire.json",
        socket_file=config_dir / "padfire.sock",
        pid_file=config_dir / "padfire.pid",
        hearth_socket=hearth_socket,
    )


def migrate_legacy_config(config_file: str | Path) -> bool:
    """Copy the historic ~/.config/roaring/padfire.json when needed."""
    destination = Path(config_file)
    old_file = destination.parent.parent / "roaring" / "padfire.json"
    if old_file.is_file() and not destination.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(old_file, destination)
        print(f"[config] migrated {old_file} -> {destination}")
        return True
    if destination.exists():
        for stale_name in ("padfire.sock", "padfire.pid"):
            try:
                (old_file.parent / stale_name).unlink(missing_ok=True)
            except OSError:
                pass
    return False


def migrate_config(config: Any) -> dict[str, Any]:
    """Normalize older settings to the current, intentionally stable v5 schema."""
    if not isinstance(config, dict):
        raise ValueError("Padfire config root must be an object")
    pages = config.get("pages")
    if not isinstance(pages, list):
        pages = []
    pages = pages[:NUM_PAGES]
    normalized_pages: list[dict[str, Any]] = []
    for page in pages:
        if not isinstance(page, dict):
            normalized_pages.append({})
            continue
        normalized_page: dict[str, Any] = {}
        for note, pad in page.items():
            if isinstance(pad, dict):
                normalized_page[str(note)] = pad
        normalized_pages.append(normalized_page)
    pages = normalized_pages
    while len(pages) < NUM_PAGES:
        pages.append({})
    config["pages"] = pages

    version = config.get("version", 1)
    try:
        version = int(version)
    except (TypeError, ValueError):
        version = 1
    if version < 4:
        config.setdefault("monitor_enabled", False)
        config.setdefault("monitor_sink", "")
        for page in pages:
            if isinstance(page, dict):
                for pad in page.values():
                    if isinstance(pad, dict):
                        pad.setdefault("action", "play")
    if config.get("retrigger") not in ("restart", "toggle"):
        config["retrigger"] = "restart"
    config.pop("device", None)
    for page in pages:
        if isinstance(page, dict):
            for pad in page.values():
                if isinstance(pad, dict):
                    pad.pop("mode", None)
    config["version"] = CONFIG_VERSION
    return config


def load_config_file(
    config_file: str | Path,
    *,
    windows: bool | None = None,
) -> dict[str, Any]:
    """Load/migrate JSON, returning safe defaults on unreadable user data."""
    try:
        value = json.loads(Path(config_file).read_text(encoding="utf-8"))
        return migrate_config(value)
    except (OSError, ValueError, TypeError, json.JSONDecodeError):
        return default_config(windows=windows)


def save_config_file(config: dict[str, Any], config_file: str | Path) -> None:
    """Atomically replace config and retain one previous generation as .bak."""
    destination = Path(config_file)
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=destination.name + ".",
            suffix=".tmp",
            dir=destination.parent,
            delete=False,
        ) as handle:
            temporary = Path(handle.name)
            json.dump(config, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())

        backup = Path(str(destination) + ".bak")
        if destination.exists():
            try:
                shutil.copy2(destination, backup)
            except OSError as exc:
                print(f"[config] backup failed: {exc}")
        os.replace(temporary, destination)
        temporary = None

        # Opening/fsyncing a directory is POSIX-specific and is not supported
        # by Windows; the file itself has already been flushed above.
        if os.name != "nt":
            try:
                fd = os.open(str(destination.parent), os.O_RDONLY)
                try:
                    os.fsync(fd)
                finally:
                    os.close(fd)
            except OSError:
                pass
    finally:
        if temporary is not None:
            try:
                temporary.unlink(missing_ok=True)
            except OSError:
                pass


def validate_pad_config(config: dict[str, Any]) -> list[str]:
    """Return non-fatal schema warnings suitable for a diagnostics panel."""
    warnings: list[str] = []
    if config.get("version") != CONFIG_VERSION:
        warnings.append("Configuration was not normalized to the current schema.")
    pages = config.get("pages")
    if not isinstance(pages, list) or len(pages) != NUM_PAGES:
        warnings.append("Expected exactly eight pages.")
        return warnings
    valid_actions = {"play", "script", "stop_sfx", "stop_all", "stop_key"}
    for page_index, page in enumerate(pages):
        if not isinstance(page, dict):
            warnings.append(f"Page {page_index + 1} is not an object.")
            continue
        for note, pad in page.items():
            if not isinstance(pad, dict):
                warnings.append(f"Page {page_index + 1}, pad {note}: invalid assignment.")
            elif pad.get("action", "play") not in valid_actions:
                warnings.append(f"Page {page_index + 1}, pad {note}: unknown action.")
    return warnings
