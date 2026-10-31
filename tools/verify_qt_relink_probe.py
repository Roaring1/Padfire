#!/usr/bin/env python3
"""Load a rebuilt QtCore DLL and call its Padfire relink-probe export."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path


def verify(library_path: Path, expected_marker: str) -> dict[str, str]:
    if os.name != "nt":
        raise RuntimeError("The QtCore relink probe must run on Windows.")
    path = library_path.resolve(strict=True)
    if not path.is_file():
        raise FileNotFoundError(path)
    dll_directory = os.add_dll_directory(str(path.parent))
    try:
        library = ctypes.WinDLL(str(path), use_last_error=True)
        marker_function = library.padfire_qt_relink_test_marker
        marker_function.argtypes = []
        marker_function.restype = ctypes.c_char_p
        raw_marker = marker_function()
        if raw_marker is None:
            raise RuntimeError("The QtCore marker export returned NULL.")
        marker = raw_marker.decode("ascii", errors="strict")
        if marker != expected_marker:
            raise RuntimeError(f"Unexpected QtCore marker: {marker!r}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        return {"library": str(path), "sha256": digest, "marker": marker}
    finally:
        dll_directory.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--expected-marker", required=True)
    args = parser.parse_args()
    print(json.dumps(verify(args.library, args.expected_marker), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
