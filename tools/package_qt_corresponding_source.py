#!/usr/bin/env python3
"""Fetch and verify the exact Qt/PySide sources used by the Windows build.

The source archives are stored unchanged, alongside a hash manifest and an
inventory of Qt/PySide DLLs found in the corresponding Padfire binary artifact.
This creates a corresponding-source preparation artifact; it is not a written
source offer and does not perform the LGPL relinking/installability test.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


SOURCES = (
    {
        "name": "Qt 6.12.0 full Windows source",
        "filename": "qt-everywhere-src-6.12.0.zip",
        "directory": "Qt",
        "size": 604_643_023,
        "sha256": "a72e89c33726d742e67edf494abf7b625eaffc756673221c9823322ecd1a32d1",
        "urls": (
            "https://download.qt.io/official_releases/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip",
            "https://ftp.fau.de/qtproject/archive/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip",
            "https://mirror.accum.se/mirror/qt.io/qtproject/archive/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip",
        ),
        "official_info": "https://download.qt.io/official_releases/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip.mirrorlist",
    },
    {
        "name": "PySide6 and Shiboken6 6.12.0 source",
        "filename": "pyside-setup-everywhere-src-6.12.0.tar.xz",
        "directory": "PySide",
        "size": 18_211_772,
        "sha256": "099c1a597cf33c1b000e11cf68a0b82d216fb802bf81726efc720394f10c7751",
        "urls": (
            "https://download.qt.io/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz",
            "https://ftp.fau.de/qtproject/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz",
            "https://mirror.accum.se/mirror/qt.io/qtproject/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz",
        ),
        "official_info": "https://download.qt.io/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz.mirrorlist",
    },
)

BUNDLED_QT_FILES = (
    "Qt6Core.dll",
    "Qt6Gui.dll",
    "Qt6Network.dll",
    "Qt6Svg.dll",
    "Qt6Widgets.dll",
    "pyside6.abi3.dll",
    "plugins/generic/qtuiotouchplugin.dll",
    "plugins/iconengines/qsvgicon.dll",
    "plugins/imageformats/qgif.dll",
    "plugins/imageformats/qicns.dll",
    "plugins/imageformats/qico.dll",
    "plugins/imageformats/qjpeg.dll",
    "plugins/imageformats/qsvg.dll",
    "plugins/imageformats/qtga.dll",
    "plugins/imageformats/qtiff.dll",
    "plugins/imageformats/qwbmp.dll",
    "plugins/imageformats/qwebp.dll",
    "plugins/networkinformation/qnetworklistmanager.dll",
    "plugins/platforms/qdirect2d.dll",
    "plugins/platforms/qminimal.dll",
    "plugins/platforms/qoffscreen.dll",
    "plugins/platforms/qwindows.dll",
    "plugins/styles/qmodernwindowsstyle.dll",
    "plugins/tls/qcertonlybackend.dll",
    "plugins/tls/qopensslbackend.dll",
    "plugins/tls/qschannelbackend.dll",
)


def sha256_file(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def download_verified(source: dict, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    for url in source["urls"]:
        partial = destination.with_suffix(destination.suffix + ".part")
        digest = hashlib.sha256()
        total = 0
        try:
            request = urllib.request.Request(
                url,
                headers={"User-Agent": "Padfire-Qt-Source-Packager/1.0"},
            )
            with urllib.request.urlopen(request, timeout=90) as response, partial.open("wb") as out:
                while chunk := response.read(4 * 1024 * 1024):
                    out.write(chunk)
                    digest.update(chunk)
                    total += len(chunk)
                    print(
                        f"{source['filename']}: {total:,}/{source['size']:,} bytes",
                        flush=True,
                    )
            actual = digest.hexdigest()
            if total != source["size"] or actual.lower() != source["sha256"]:
                raise ValueError(
                    f"checksum/size mismatch: bytes={total}, SHA256={actual}"
                )
            partial.replace(destination)
            print(f"VERIFIED {destination.name}: SHA256 {actual}", flush=True)
            return
        except (OSError, urllib.error.URLError, TimeoutError, ValueError) as exc:
            errors.append(f"{url}: {exc}")
            partial.unlink(missing_ok=True)
            print(f"Download attempt failed: {errors[-1]}", file=sys.stderr, flush=True)
    raise RuntimeError(
        f"Could not fetch verified source {source['filename']}:\n"
        + "\n".join(errors)
    )


def write_manifest(output: Path, binary_inventory: Path | None) -> None:
    lines = [
        "# Padfire Windows — Qt/PySide corresponding source archive",
        "",
        "These are the unchanged, version-matched upstream source archives for",
        "the Qt and Qt for Python libraries bundled by the Windows 6.12.0 build.",
        "They are included to make the LGPL corresponding-source material",
        "available for review and release packaging. This package is not a",
        "distributor-controlled written offer and does not prove successful",
        "library replacement/relinking or installation on a user's PC.",
        "",
        "## Source archives",
        "",
    ]
    for source in SOURCES:
        rel = f"{source['directory']}/{source['filename']}"
        lines.extend(
            [
                f"### {source['name']}",
                "",
                f"- Archive: `{rel}`",
                f"- Exact size: {source['size']:,} bytes",
                f"- SHA-256: `{source['sha256']}`",
                f"- Upstream checksum/source record: {source['official_info']}",
                f"- Official source download:",
                f"  {source['urls'][0]}",
                "",
            ]
        )
    lines.extend(
        [
            "## Qt/PySide runtime components in Padfire's Windows package",
            "",
            "The audited 6.12.0 binary package includes the following Qt/PySide",
            "DLLs and plugins. The Qt source archive contains the Qt module/plugin",
            "sources and their individual third-party license material; the PySide",
            "source archive contains the binding/Shiboken superproject sources.",
            "Review each bundled plugin and third-party dependency before choosing",
            "a distribution licensing basis.",
            "",
        ]
    )
    lines.extend(f"- `{item}`" for item in BUNDLED_QT_FILES)
    lines.extend(
        [
            "",
            "## Build correspondence and limitations",
            "",
            "- Binary package Qt runtime: 6.12.0",
            "- PySide6-Essentials: 6.12.0",
            "- Shiboken6: 6.12.0",
            "- Windows architecture: x86-64",
            "- Python runtime: CPython 3.12.10 x64",
            "- PySide6 wheel metadata declares: LGPL-3.0-only OR GPL-2.0-only",
            "  OR GPL-3.0-only. The current project selects LGPLv3 for distribution",
            "  of its Qt/PySide runtime, subject to full LGPL compliance.",
            "- The binary contains dynamically loaded Qt DLLs under",
            "  `_internal/PySide6/`; the source code for the Python application is",
            "  available in the Padfire repository.",
            "- A compatible replacement/relink build must use the matching Qt",
            "  and PySide ABI. These source archives alone do not supply the build",
            "  toolchain, all binary build flags, or installation information.",
            "",
            "## Before redistribution",
            "",
            "The release owner must still validate the source correspondence for",
            "every distributed Qt DLL/plugin, keep source available with the app",
            "or provide a legally valid written offer controlled by the distributor,",
            "and demonstrate that users can replace the LGPL libraries, relink,",
            "and run the application with required installation information.",
            "The one-folder package layout is not by itself proof of compliance.",
            "",
            "See the repository file `WINDOWS_QT_SOURCE_COMPLIANCE.md` and Qt's",
            "official obligations: https://www.qt.io/development/open-source-lgpl-obligations",
            "",
        ]
    )
    manifest = output / "SOURCE_MANIFEST.md"
    manifest.write_text("\n".join(lines), encoding="utf-8")

    checksums = [
        f"{source['sha256']}  {source['directory']}/{source['filename']}"
        for source in SOURCES
    ]
    (output / "SHA256SUMS.txt").write_text("\n".join(checksums) + "\n", encoding="ascii")
    if binary_inventory and binary_inventory.exists():
        shutil.copy2(binary_inventory, output / "BINARY_COMPONENTS.md")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary-inventory", type=Path)
    parser.add_argument("--manifest-only", action="store_true")
    args = parser.parse_args()

    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for source in SOURCES:
        destination = output / source["directory"] / source["filename"]
        if not args.manifest_only:
            if destination.exists():
                size, digest = sha256_file(destination)
                if size != source["size"] or digest.lower() != source["sha256"]:
                    destination.unlink()
                    download_verified(source, destination)
                else:
                    print(f"Already verified: {destination}", flush=True)
            else:
                download_verified(source, destination)
    write_manifest(output, args.binary_inventory)
    for source in SOURCES:
        path = output / source["directory"] / source["filename"]
        if not args.manifest_only and not path.is_file():
            raise FileNotFoundError(path)
    print(f"Verified Qt/PySide source package at {output}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
