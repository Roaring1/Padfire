#!/usr/bin/env python3
"""Windowed Windows entry point; diagnostics are written to a rotating user log."""
from __future__ import annotations

import sys

if __name__ == "__main__":
    from padfire_win.logging_setup import configure_windowed_logging
    from padfire_win.app import main

    if sys.stdout is None or sys.stderr is None:
        configure_windowed_logging()
    raise SystemExit(main())
