#!/usr/bin/env python3
"""Rebind NumPy's hash-renamed MSVC runtime imports to the system VC++ runtime."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import importlib.metadata as metadata
import os
import re
import shutil
import subprocess
import sys
import sysconfig
from pathlib import Path

import pefile

HASHED_MSVC_DLL = re.compile(r"^msvcp140-[0-9a-f]{32}\.dll$", re.IGNORECASE)
SYSTEM_IMPORT_NAME = "msvcp140.dll"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def pe_import_names(path: Path) -> set[str]:
    pe = pefile.PE(str(path), fast_load=False)
    try:
        names: set[str] = set()
        for directory in ("DIRECTORY_ENTRY_IMPORT", "DIRECTORY_ENTRY_DELAY_IMPORT"):
            for entry in (getattr(pe, directory, None) or []):
                raw = getattr(entry, "dll", None)
                if raw:
                    names.add(raw.decode("ascii", errors="replace").rstrip("\x00").casefold())
        return names
    finally:
        pe.close()


def scan_importers(site_packages: Path, wanted: set[str]) -> dict[str, list[Path]]:
    result = {name: [] for name in wanted}
    for path in site_packages.rglob("*"):
        if not path.is_file() or path.suffix.casefold() not in {".pyd", ".dll", ".exe"}:
            continue
        try:
            imports = pe_import_names(path)
        except pefile.PEFormatError:
            continue
        for name in wanted.intersection(imports):
            result[name].append(path)
    return result


def numeric_smoke_test() -> str:
    probe = (
        "import numpy as np; import scipy; from scipy.fft import fft; "
        "assert int(np.dot(np.array([1, 2]), np.array([3, 4]))) == 11; "
        "assert abs(float(fft([1.0, 0.0, 0.0, 0.0])[0].real) - 1.0) < 1e-9; "
        "from scipy.linalg import norm; assert abs(float(norm([3.0, 4.0])) - 5.0) < 1e-9; "
        "print('NumPy/SciPy system-CRT import and numeric smoke tests passed')"
    )
    result = subprocess.run(
        [sys.executable, "-c", probe],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def normalize(output: Path) -> Path:
    if os.name != "nt":
        raise RuntimeError("NumPy MSVC import normalization must run on Windows.")
    try:
        ctypes.WinDLL(SYSTEM_IMPORT_NAME)
    except OSError as exc:
        raise RuntimeError("The official system msvcp140.dll is not loadable.") from exc

    tool_version = metadata.version("delvewheel")
    if tool_version != "1.13.2":
        raise RuntimeError(f"Expected delvewheel 1.13.2; found {tool_version!r}.")
    tool = shutil.which("delvewheel")
    if not tool:
        raise RuntimeError("Pinned delvewheel command was not found on PATH.")

    site_packages = Path(sysconfig.get_paths()["purelib"]).resolve()
    numpy_version = metadata.version("numpy")
    numpy_libs = site_packages / "numpy.libs"
    candidates = sorted(
        p for p in numpy_libs.glob("msvcp140-*.dll")
        if HASHED_MSVC_DLL.fullmatch(p.name)
    )
    if not candidates:
        raise RuntimeError(
            f"NumPy {numpy_version} has no expected hashed MSVC dependency. "
            "Re-audit the new wheel instead of silently skipping the patch."
        )

    target_names = {p.name.casefold() for p in candidates}
    importer_map = scan_importers(site_packages, target_names)
    for candidate in candidates:
        if not importer_map[candidate.name.casefold()]:
            raise RuntimeError(f"No PE importers found for {candidate.name}.")

    importers = sorted(
        {p for group in importer_map.values() for p in group},
        key=lambda p: p.as_posix().casefold(),
    )
    original_importer_bytes = {p: p.read_bytes() for p in importers}
    original_runtime_bytes = {p: p.read_bytes() for p in candidates}
    original_runtime_hashes = {p.name: sha256(p) for p in candidates}
    before_hashes = {p: sha256(p) for p in importers}
    changes: list[tuple[Path, str]] = []

    try:
        for candidate in candidates:
            old_name = candidate.name
            for importer in importer_map[old_name.casefold()]:
                subprocess.run(
                    [tool, "replace-needed", str(importer), "-change", old_name, SYSTEM_IMPORT_NAME],
                    check=True,
                    capture_output=True,
                    text=True,
                )
                changes.append((importer, old_name))

        remaining = scan_importers(site_packages, target_names)
        unresolved = {name: files for name, files in remaining.items() if files}
        if unresolved:
            detail = "; ".join(
                f"{name}: {', '.join(p.relative_to(site_packages).as_posix() for p in files)}"
                for name, files in unresolved.items()
            )
            raise RuntimeError("Old NumPy MSVC imports remain: " + detail)

        for importer, old_name in changes:
            imports = pe_import_names(importer)
            if old_name.casefold() in imports:
                raise RuntimeError(f"Old import remains in {importer}")
            if SYSTEM_IMPORT_NAME not in imports:
                raise RuntimeError(f"Expected {importer} to import {SYSTEM_IMPORT_NAME}")

        for candidate in candidates:
            candidate.unlink()

        # A fresh process verifies NumPy/SciPy work after the wheel DLL is deleted.
        smoke_output = numeric_smoke_test()
        if not smoke_output:
            raise RuntimeError("NumPy/SciPy smoke test returned no success output.")
    except Exception:
        for path, data in original_importer_bytes.items():
            path.write_bytes(data)
        for path, data in original_runtime_bytes.items():
            path.write_bytes(data)
        raise

    output.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "# Windows NumPy MSVC runtime import patch",
        "",
        "Generated for this build by tools/normalize_numpy_msvc_runtime.py.",
        "",
        f"- NumPy version: {numpy_version}",
        f"- delvewheel version: {tool_version}",
        f"- Hash-renamed wheel runtime DLLs removed: {len(candidates)}",
        f"- System import name: {SYSTEM_IMPORT_NAME}",
        "- Only PE import descriptor names changed; the Microsoft runtime DLL itself was not modified or copied.",
        "- Required target prerequisite: Microsoft Visual C++ x64 Redistributable 14.44.35211.0 or newer.",
        f"- Fresh-process numeric test: {smoke_output}",
        "",
        "## Removed wheel runtime files",
        "",
        "| Original file | SHA-256 |",
        "|---|---|",
    ]
    for name, digest in sorted(original_runtime_hashes.items()):
        lines.append(f"| {name} | {digest} |")
    lines.extend([
        "",
        "## NumPy PE importers",
        "",
        "| File | Original SHA-256 | Patched SHA-256 | Previous import |",
        "|---|---|---|---|",
    ])
    for importer, old_name in changes:
        relative = importer.relative_to(site_packages).as_posix()
        lines.append(f"| {relative} | {before_hashes[importer]} | {sha256(importer)} | {old_name} |")
    lines.extend([
        "",
        "The hash-renamed DLL is not included in either Padfire bundle.",
        "The NumPy extension modules import the system-provided msvcp140.dll.",
        "This is a deterministic build-time binary patch; it does not modify NumPy source code.",
    ])
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("packaging/windows/generated_licenses/WINDOWS_RUNTIME_PATCHES.md"),
    )
    args = parser.parse_args()
    manifest = normalize(args.output)
    print(f"NumPy runtime imports normalized; manifest: {manifest.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
