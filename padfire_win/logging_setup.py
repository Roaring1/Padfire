"""Small rotating log sink used by the windowed Windows executable."""
from __future__ import annotations

import logging
from logging.handlers import RotatingFileHandler
from pathlib import Path
from typing import TextIO

from padfire_core import get_paths


class _LoggerStream:
    def __init__(self, logger: logging.Logger) -> None:
        self._logger = logger

    def write(self, value: str) -> int:
        message = value.rstrip()
        if message:
            self._logger.info(message)
        return len(value)

    def flush(self) -> None:
        for handler in logging.getLogger().handlers:
            handler.flush()


def configure_windowed_logging() -> None:
    paths = get_paths()
    log_dir = paths.pid_file.parent / "Logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    handler = RotatingFileHandler(
        log_dir / "padfire.log", maxBytes=1_000_000, backupCount=3, encoding="utf-8"
    )
    logging.basicConfig(
        level=logging.INFO,
        handlers=[handler],
        format="%(asctime)s %(levelname)s %(message)s",
    )
    import sys

    logger = logging.getLogger("padfire.window")
    sys.stdout = _LoggerStream(logger)  # type: ignore[assignment]
    sys.stderr = _LoggerStream(logger)  # type: ignore[assignment]
