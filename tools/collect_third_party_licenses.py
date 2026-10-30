#!/usr/bin/env python3
"""Collect license files and version metadata for Padfire's Windows runtime.

Run after installing packaging/windows/requirements.txt and before PyInstaller.
The output is copied into each one-folder distribution under ThirdPartyLicenses/.
"""
from __future__ import annotations

import argparse
import importlib.metadata as metadata
import re
import shutil
import sys
import platform
from pathlib import Path, PurePosixPath

from packaging.markers import default_environment
from packaging.requirements import Requirement


RUNTIME_ROOTS = (
    "PySide6-Essentials",
    "mido",
    "python-rtmidi",
    "numpy",
    "sounddevice",
    "soundfile",
    "pyloudnorm",
    "PyInstaller",
)
LICENSE_PREFIXES = (
    "license",
    "licence",
    "copying",
    "notice",
    "copyright",
    "patents",
)
MAX_LICENSE_FILE_BYTES = 20 * 1024 * 1024


def safe_name(value: str) -> str:
    value = re.sub(r"[^A-Za-z0-9._+-]+", "_", value.strip())
    return value.strip("._") or "package"


def runtime_distributions() -> list[metadata.Distribution]:
    """Resolve runtime dependency closure using the active environment markers."""
    environment = default_environment()
    environment["extra"] = ""
    queue = list(RUNTIME_ROOTS)
    found: dict[str, metadata.Distribution] = {}

    while queue:
        requested = queue.pop(0)
        key = re.sub(r"[-_.]+", "-", requested).casefold()
        if key in found:
            continue
        try:
            distribution = metadata.distribution(requested)
        except metadata.PackageNotFoundError as exc:
            raise RuntimeError(
                f"Required runtime distribution is not installed: {requested}"
            ) from exc

        actual_name = distribution.metadata.get("Name") or requested
        actual_key = re.sub(r"[-_.]+", "-", actual_name).casefold()
        if actual_key in found:
            continue
        found[actual_key] = distribution

        for raw_requirement in distribution.requires or ():
            requirement = Requirement(raw_requirement)
            if requirement.marker is not None and not requirement.marker.evaluate(environment):
                continue
            dependency_key = re.sub(r"[-_.]+", "-", requirement.name).casefold()
            if dependency_key not in found:
                queue.append(requirement.name)

    return sorted(
        found.values(),
        key=lambda dist: (dist.metadata.get("Name") or "").casefold(),
    )


def is_license_path(relative: PurePosixPath) -> bool:
    """Recognize license directories and conventional names, not code modules."""
    parts = [part.casefold() for part in relative.parts]
    filename = relative.name.casefold()

    # Some packages have an importable Python module literally named
    # licenses (for example packaging/licenses/_spdx.py). Those files
    # describe or parse SPDX identifiers; they are not license texts and
    # must not be copied into the third-party notice inventory as such.
    if relative.suffix.casefold() in {
        ".py", ".pyc", ".pyo", ".pyi", ".so", ".pyd", ".dll", ".exe"
    }:
        return False

    return "licenses" in parts or filename.startswith(LICENSE_PREFIXES)

def license_files(distribution: metadata.Distribution) -> list[tuple[object, Path]]:
    """Find wheel-provided license/notice files without guessing their contents."""
    candidates: dict[str, tuple[object, Path]] = {}
    for package_path in distribution.files or ():
        relative = PurePosixPath(str(package_path))
        if not is_license_path(relative):
            continue
        source = Path(distribution.locate_file(package_path))
        try:
            if not source.is_file() or source.stat().st_size > MAX_LICENSE_FILE_BYTES:
                continue
        except OSError:
            continue
        candidates[str(relative)] = (package_path, source)
    return [candidates[key] for key in sorted(candidates)]


def component_license_notes(name: str, license_expression: str) -> list[str]:
    """Explain PySide/Shiboken's multi-option metadata and wheel notice accurately."""
    key = re.sub(r"[-_.]+", "-", name).casefold()
    if key not in {"pyside6-essentials", "shiboken6"}:
        return []

    expected = "LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only"
    if license_expression != expected:
        raise RuntimeError(
            f"Unreviewed {name} license expression: {license_expression!r}. "
            "Re-audit the wheel license options and notice files before packaging."
        )

    return [
        "- **Build-selected option:** Padfire's Windows community build selects "
        "`LGPL-3.0-only` from the alternatives in the wheel metadata; see "
        "`THIRD_PARTY_NOTICES.md`.",
        "- **Wheel notice clarification:** the included "
        "`LicenseRef-Qt-Commercial.txt` is a short reference for licensees with "
        "separate commercial agreements. It is not the LGPL text and is not "
        "evidence that this build selected the commercial option.",
        "- The complete `LGPL-3.0-only.txt` text is included beside this inventory. "
        "The selected license path does not by itself establish corresponding-"
        "source, relinking/replacement, or installation-information compliance."
    ]

def collect(output: Path) -> tuple[int, int]:
    output = output.resolve()
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)

    distributions = runtime_distributions()
    manifest_lines = [
        "# Windows runtime component inventory",
        "",
        "Generated from the exact Python environment used for this build.",
        "This inventory records package metadata and copies license/notice files",
        "present in the installed wheels. A missing file is explicitly flagged",
        "and must be resolved before public redistribution.",
        "",
    ]
    copied_count = 1
    missing_count = 0
    python_license = collect_python_license(output)
    manifest_lines.extend(
        [
            f"## CPython {platform.python_version()}",
            "",
            "- License metadata: Python Software Foundation License 2.0",
            f"- Included: `{python_license.relative_to(output).as_posix()}`",
            "",
        ]
    )

    for distribution in distributions:
        name = distribution.metadata.get("Name") or "Unknown"
        version = distribution.version
        license_expression = (
            distribution.metadata.get("License-Expression")
            or distribution.metadata.get("License")
            or "Not declared in package metadata"
        )
        package_dir = output / f"{safe_name(name)}-{safe_name(version)}"
        files = license_files(distribution)

        manifest_lines.extend(
            [
                f"## {name} {version}",
                "",
                f"- License metadata: {license_expression}",
            ]
        )
        manifest_lines.extend(component_license_notes(name, license_expression))
        project_urls = distribution.metadata.get_all("Project-URL") or []
        home_page = distribution.metadata.get("Home-page")
        if home_page:
            manifest_lines.append(f"- Project: {home_page}")
        for project_url in project_urls:
            manifest_lines.append(f"- {project_url}")

        if files:
            package_dir.mkdir(parents=True, exist_ok=True)
            used_names: set[str] = set()
            for _package_path, source in files:
                destination_name = source.name
                if destination_name.casefold() in used_names:
                    destination_name = f"{len(used_names) + 1}-{destination_name}"
                used_names.add(destination_name.casefold())
                destination = package_dir / destination_name
                shutil.copy2(source, destination)
                copied_count += 1
                manifest_lines.append(
                    f"- Included: `{package_dir.name}/{destination.name}`"
                )
        else:
            missing_count += 1
            package_dir.mkdir(parents=True, exist_ok=True)
            marker = package_dir / "LICENSE_FILE_NOT_INCLUDED.txt"
            marker.write_text(
                "The installed wheel did not expose a matching license/notice file.\n"
                "Obtain the exact license and copyright notices for this component\n"
                "before redistributing a binary that includes it.\n",
                encoding="utf-8",
            )
            manifest_lines.append(
                f"- **MISSING FROM WHEEL:** no license/notice file was found; "
                f"see `{package_dir.name}/{marker.name}`."
            )
        manifest_lines.append("")

    manifest = output / "COMPONENTS.md"
    manifest.write_text("\n".join(manifest_lines).rstrip() + "\n", encoding="utf-8")
    return copied_count, missing_count



def collect_python_license(output: Path) -> Path:
    """Include the license for the CPython runtime embedded by PyInstaller."""
    candidates = (
        Path(sys.base_prefix) / "LICENSE.txt",
        Path(sys.base_prefix) / "LICENSE",
        Path(sys.prefix) / "LICENSE.txt",
        Path(sys.prefix) / "LICENSE",
    )
    source = next((path for path in candidates if path.is_file()), None)
    if source is None:
        raise RuntimeError(
            "Could not find the Python Software Foundation license beside the "
            f"build interpreter (sys.base_prefix={sys.base_prefix!r})."
        )
    destination = output / f"Python-{safe_name(platform.python_version())}"
    destination.mkdir(parents=True, exist_ok=True)
    target = destination / "PSF-2.0.txt"
    shutil.copy2(source, target)
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("packaging/windows/generated_licenses"),
        help="output directory for third-party license files",
    )
    parser.add_argument(
        "--fail-on-missing",
        action="store_true",
        help="return a failure when any runtime distribution has no bundled license file",
    )
    args = parser.parse_args()
    copied, missing = collect(args.output)
    print(f"Collected {copied} license/notice files; {missing} distributions need review.")
    print(f"Manifest: {args.output.resolve() / 'COMPONENTS.md'}")
    if args.fail_on_missing and missing:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
