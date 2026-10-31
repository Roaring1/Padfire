#!/usr/bin/env python3
"""Stage legal notices, validate NumPy PE imports, and remove app-local Microsoft CRT copies."""
from __future__ import annotations
import argparse
import shutil
from pathlib import Path

import pefile

ROOT_FILES = ("LICENSE", "THIRD_PARTY_NOTICES.md", "WINDOWS_RUNTIME_REQUIREMENTS.md")
AMBIENT_OPENSSL_DLLS = {"libcrypto-3-x64.dll", "libssl-3-x64.dll"}
# Qt 6.12 says loose compiler runtimes picked up from developer machines are
# not intended/licensed for redistribution. Require the official system VC++
# Redistributable. NumPy's hashed import name is normalized before PyInstaller.
APP_LOCAL_RUNTIME_DLLS = {
    "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll",
    "vcruntime140.dll", "vcruntime140_1.dll", "ucrtbase.dll",
}

EXPECTED_NUMPY_IMPORTERS = {
    "_multiarray_umath.cp312-win_amd64.pyd",
    "_pocketfft_umath.cp312-win_amd64.pyd",
}


def pe_import_names(path: Path) -> set[str]:
    """Read normal and delay-load imports from a PE binary."""
    pe = pefile.PE(str(path), fast_load=False)
    try:
        result: set[str] = set()
        for directory in ("DIRECTORY_ENTRY_IMPORT", "DIRECTORY_ENTRY_DELAY_IMPORT"):
            for entry in (getattr(pe, directory, None) or []):
                raw = getattr(entry, "dll", None)
                if raw:
                    result.add(raw.decode("ascii", errors="replace").rstrip("\\x00").casefold())
        return result
    finally:
        pe.close()


def verify_numpy_system_runtime_imports(bundle: Path) -> None:
    """Require both patched NumPy extensions to import the system MSVC runtime."""
    for filename in sorted(EXPECTED_NUMPY_IMPORTERS):
        matches = list(bundle.rglob(filename))
        if len(matches) != 1:
            raise RuntimeError(
                f"Expected exactly one packaged {filename}; found {len(matches)}."
            )
        imports = pe_import_names(matches[0])
        if "msvcp140.dll" not in imports:
            raise RuntimeError(
                f"Packaged {matches[0].relative_to(bundle)} does not import system msvcp140.dll."
            )

    # Scan every final PE image so no private hash-renamed runtime import
    # can be reintroduced by another NumPy module or PyInstaller hook.
    private_imports: list[str] = []
    for candidate in bundle.rglob("*"):
        if not candidate.is_file() or candidate.suffix.casefold() not in {".pyd", ".dll", ".exe"}:
            continue
        try:
            imports = pe_import_names(candidate)
        except pefile.PEFormatError:
            continue
        for name in imports:
            folded = name.casefold()
            if folded.startswith("msvcp140-") and folded.endswith(".dll"):
                private_imports.append(
                    f"{candidate.relative_to(bundle).as_posix()} -> {name}"
                )
    if private_imports:
        raise RuntimeError(
            "Private hash-renamed Microsoft CRT imports remain: " + "; ".join(private_imports)
        )


def normalize(bundle: Path, source_root: Path) -> None:
    bundle = bundle.resolve()
    if not bundle.is_dir():
        raise FileNotFoundError(f"PyInstaller bundle does not exist: {bundle}")
    for name in ROOT_FILES:
        source = source_root / name
        if not source.is_file():
            raise FileNotFoundError(f"Repository legal/runtime file is missing: {source}")
        shutil.copy2(source, bundle / name)

    qt_licenses_source = source_root / "licenses" / "QtThirdParty"
    if not (qt_licenses_source / "README.md").is_file():
        raise FileNotFoundError(f"Qt third-party notices are missing: {qt_licenses_source}")

    removed_openssl, removed_runtime = [], []
    for candidate in bundle.rglob("*.dll"):
        name = candidate.name.casefold()
        if name in AMBIENT_OPENSSL_DLLS:
            candidate.unlink()
            removed_openssl.append(candidate.relative_to(bundle).as_posix())
        elif name in APP_LOCAL_RUNTIME_DLLS:
            candidate.unlink()
            removed_runtime.append(candidate.relative_to(bundle).as_posix())
    if removed_openssl:
        print("Removed ambient OpenSSL DLLs: " + ", ".join(removed_openssl), flush=True)
    if removed_runtime:
        print("Removed app-local Microsoft CRT/UCRT DLLs: " + ", ".join(removed_runtime), flush=True)

    remaining = [
        p.relative_to(bundle).as_posix() for p in bundle.rglob("*.dll")
        if p.name.casefold() in APP_LOCAL_RUNTIME_DLLS
        or (p.name.casefold().startswith("msvcp140-") and p.name.casefold().endswith(".dll"))
    ]
    if remaining:
        raise RuntimeError("App-local or hash-renamed Microsoft CRT DLLs remain: " + ", ".join(remaining))

    verify_numpy_system_runtime_imports(bundle)

    destination = bundle / "ThirdPartyLicenses"
    candidates = (destination, bundle / "_internal" / "ThirdPartyLicenses")
    license_source = next((x for x in candidates if x.is_dir()), None)
    if license_source is None:
        raise FileNotFoundError(f"No ThirdPartyLicenses directory found in {bundle}")
    if license_source.resolve() != destination.resolve():
        shutil.copytree(license_source, destination, dirs_exist_ok=True)
        shutil.rmtree(license_source)

    shutil.copytree(qt_licenses_source, destination / "QtThirdParty", dirs_exist_ok=True)
    if not (destination / "COMPONENTS.md").is_file():
        raise FileNotFoundError(f"Component inventory missing from {destination}")
    if not (destination / "WINDOWS_RUNTIME_PATCHES.md").is_file():
        raise FileNotFoundError(f"NumPy runtime patch manifest missing from {destination}")
    if not (destination / "QtThirdParty" / "README.md").is_file():
        raise FileNotFoundError("Qt third-party candidate notice README missing from bundle")


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
