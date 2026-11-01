#!/usr/bin/env python3
"""Console entry point for Padfire's Windows control commands."""
import sys

from padfire_win.app import main

if __name__ == "__main__":
    if len(sys.argv) == 1:
        sys.argv.append("--help")
    raise SystemExit(main())
