#!/usr/bin/env python3
"""Add a harmless exported marker to QtCore for a modified-library relink test."""
from __future__ import annotations

import argparse
import difflib
import hashlib
from pathlib import Path

ANCHOR = (
    "const char *qVersion() noexcept\n"
    "{\n"
    "    return QT_VERSION_STR;\n"
    "}\n"
)
MARKER = "PADFIRE-QT-RELINK-TEST-6.12.0"
INSERTION = (
    '\nextern "C" Q_CORE_EXPORT const char *padfire_qt_relink_test_marker() noexcept\n'
    "{\n"
    f'    return "{MARKER}";\n'
    "}\n"
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def apply_patch(source_root: Path, patch_output: Path | None = None) -> tuple[str, str]:
    source = source_root.resolve() / "qtbase" / "src" / "corelib" / "global" / "qlibraryinfo.cpp"
    if not source.is_file():
        raise FileNotFoundError(source)
    original_bytes = source.read_bytes()
    original_text = original_bytes.decode("utf-8")
    normalized = original_text.replace("\r\n", "\n")
    if normalized.count(ANCHOR) != 1:
        raise RuntimeError(
            "Expected the unmodified Qt 6.12.0 qVersion() definition exactly once; "
            f"found {normalized.count(ANCHOR)} occurrences."
        )
    if MARKER in normalized or "padfire_qt_relink_test_marker" in normalized:
        raise RuntimeError("The Padfire relink marker already exists; refusing a repeated patch.")
    updated = normalized.replace(ANCHOR, ANCHOR + INSERTION, 1)
    updated_bytes = updated.encode("utf-8")
    temp = source.with_suffix(source.suffix + ".padfire-tmp")
    temp.write_bytes(updated_bytes)
    temp.replace(source)
    if patch_output is not None:
        diff = difflib.unified_diff(
            original_text.splitlines(keepends=True),
            updated.splitlines(keepends=True),
            fromfile="a/qtbase/src/corelib/global/qlibraryinfo.cpp",
            tofile="b/qtbase/src/corelib/global/qlibraryinfo.cpp",
        )
        patch_output.parent.mkdir(parents=True, exist_ok=True)
        patch_output.write_text("".join(diff), encoding="utf-8", newline="\n")
    return sha256(original_bytes), sha256(updated_bytes)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--patch-output", type=Path, required=True)
    args = parser.parse_args()
    before, after = apply_patch(args.source_root, args.patch_output)
    print(f"Patched QtCore relink probe in qlibraryinfo.cpp; source SHA-256 {before} -> {after}")
    print(f"Exported C marker: {MARKER}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
