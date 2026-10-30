#!/usr/bin/env python3
"""Place legal documents at the root of PyInstaller one-folder distributions."""
from __future__ import annotations

import argparse
import shutil
from pathlib import Path


ROOT_FILES = ("LICENSE", "THIRD_PARTY_NOTICES.md")
# PySide6 wheels do not ship OpenSSL for Windows. PyInstaller can nevertheless
# pick up this unrelated versioned pair from the build runner PATH. Keep the
# CPython 3.12.10 libssl-3.dll/libcrypto-3.dll pair, but do not redistribute the
# ambient -x64 DLLs; Qt can use Windows Schannel or a user-supplied OpenSSL.
AMBIENT_OPENSSL_DLLS = {"libcrypto-3-x64.dll", "libssl-3-x64.dll"}


def normalize(bundle: Path, source_root: Path) -> None:
    bundle = bundle.resolve()
    if not bundle.is_dir():
        raise FileNotFoundError(f"PyInstaller bundle does not exist: {bundle}")

    for name in ROOT_FILES:
        source = source_root / name
        if not source.is_file():
            raise FileNotFoundError(f"Repository legal file is missing: {source}")
        shutil.copy2(source, bundle / name)

    removed = []
    for candidate in bundle.rglob("*.dll"):
        if candidate.name.casefold() in AMBIENT_OPENSSL_DLLS:
            candidate.unlink()
            removed.append(candidate.relative_to(bundle).as_posix())
    if removed:
        print(
            "Removed non-wheel ambient OpenSSL DLLs: " + ", ".join(removed),
            flush=True,
        )

    destination = bundle / "ThirdPartyLicenses"
    candidates = (
        destination,
        bundle / "_internal" / "ThirdPartyLicenses",
    )
    source = next((candidate for candidate in candidates if candidate.is_dir()), None)
    if source is None:
        raise FileNotFoundError(
            f"No ThirdPartyLicenses directory found in {bundle} or its _internal folder"
        )

    if source.resolve() != destination.resolve():
        shutil.copytree(source, destination, dirs_exist_ok=True)
        shutil.rmtree(source)

    if not (destination / "COMPONENTS.md").is_file():
        raise FileNotFoundError(f"Component inventory missing from {destination}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--bundles", nargs="+", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()

    for bundle in args.bundles:
        normalize(bundle, root)
        print(f"Legal documents staged at bundle root: {bundle.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
