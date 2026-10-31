"""Tests for Windows one-folder notice staging and runtime DLL cleanup."""
from __future__ import annotations
import tempfile
import unittest
from pathlib import Path
from tools.normalize_windows_licenses import normalize
from tools.collect_third_party_licenses import component_license_notes, is_license_path


class NormalizeWindowsLicensesTests(unittest.TestCase):
    def _source(self, root: Path) -> None:
        (root / "LICENSE").write_text("MIT", encoding="utf-8")
        (root / "THIRD_PARTY_NOTICES.md").write_text("Notices", encoding="utf-8")
        (root / "WINDOWS_RUNTIME_REQUIREMENTS.md").write_text("Runtime", encoding="utf-8")
        qt = root / "licenses" / "QtThirdParty"
        (qt / "qtbase").mkdir(parents=True)
        (qt / "qtimageformats").mkdir(parents=True)
        (qt / "README.md").write_text("Candidate notices", encoding="utf-8")
        (qt / "qtbase" / "freetype-LICENSE.txt").write_text("FTL", encoding="utf-8")
        (qt / "qtimageformats" / "libwebp-BSD-3-Clause.txt").write_text("WebP", encoding="utf-8")

    def _bundle(self, root: Path):
        bundle = root / "Padfire"
        internal = bundle / "_internal"
        legal = internal / "ThirdPartyLicenses"
        legal.mkdir(parents=True)
        (legal / "COMPONENTS.md").write_text("Components", encoding="utf-8")
        return bundle, internal

    def test_ambient_openssl_removed_and_cpython_pair_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self._source(root)
            bundle, internal = self._bundle(root)
            for name in ("libcrypto-3-x64.dll", "libssl-3-x64.dll", "libcrypto-3.dll", "libssl-3.dll"):
                (internal / name).write_bytes(b"test binary")
            normalize(bundle, root)
            self.assertFalse((internal / "libcrypto-3-x64.dll").exists())
            self.assertFalse((internal / "libssl-3-x64.dll").exists())
            self.assertTrue((internal / "libcrypto-3.dll").is_file())
            self.assertTrue((internal / "libssl-3.dll").is_file())
            self.assertTrue((bundle / "WINDOWS_RUNTIME_REQUIREMENTS.md").is_file())
            self.assertTrue((bundle / "ThirdPartyLicenses" / "QtThirdParty" / "README.md").is_file())
            self.assertTrue((bundle / "ThirdPartyLicenses" / "QtThirdParty" / "qtbase" / "freetype-LICENSE.txt").is_file())

    def test_standard_app_local_crt_removed_but_numpy_hashed_dll_is_kept(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self._source(root)
            bundle, internal = self._bundle(root)
            standard = (
                ("VCRUNTIME140.dll",), ("VCRUNTIME140_1.dll",), ("ucrtbase.dll",),
                ("PySide6", "MSVCP140.dll"), ("PySide6", "MSVCP140_1.dll"),
                ("PySide6", "MSVCP140_2.dll"), ("shiboken6", "MSVCP140.dll"),
                ("shiboken6", "VCRUNTIME140.dll"), ("shiboken6", "VCRUNTIME140_1.dll"),
            )
            for parts in standard:
                path = internal.joinpath(*parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"runtime")
            numpy_dll = internal / "numpy.libs" / "msvcp140-a4c2229bdc2a2a630acdc095b4d86008.dll"
            numpy_dll.parent.mkdir(parents=True)
            numpy_dll.write_bytes(b"NumPy wheel import")
            normalize(bundle, root)
            forbidden = {
                "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll",
                "vcruntime140.dll", "vcruntime140_1.dll", "ucrtbase.dll",
            }
            self.assertFalse([p for p in bundle.rglob("*.dll") if p.name.casefold() in forbidden])
            self.assertTrue(numpy_dll.is_file())

    def test_qt_candidate_notices_are_staged(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self._source(root)
            bundle, _internal = self._bundle(root)
            normalize(bundle, root)
            staged = bundle / "ThirdPartyLicenses" / "QtThirdParty"
            self.assertTrue((staged / "README.md").is_file())
            self.assertTrue((staged / "qtbase" / "freetype-LICENSE.txt").is_file())
            self.assertTrue((staged / "qtimageformats" / "libwebp-BSD-3-Clause.txt").is_file())


class LicensePathDiscoveryTests(unittest.TestCase):
    def test_license_directory_files_are_discovered(self) -> None:
        self.assertTrue(is_license_path(Path("LICENSES/LGPL-3.0-only.txt")))
        self.assertTrue(is_license_path(Path("pkg.dist-info/licenses/LicenseRef-Qt-Commercial.txt")))
        self.assertTrue(is_license_path(Path("LICENSE")))
        self.assertFalse(is_license_path(Path("PySide6/Qt6Core.dll")))

    def test_python_modules_inside_license_named_package_are_not_notices(self) -> None:
        self.assertFalse(is_license_path(Path("packaging/licenses/__init__.py")))
        self.assertFalse(is_license_path(Path("packaging/licenses/_spdx.py")))
        self.assertFalse(is_license_path(Path("packaging/licenses/_spdx.pyi")))
        self.assertFalse(is_license_path(Path("packaging/licenses/__pycache__/_spdx.cpython-312.pyc")))
        self.assertTrue(is_license_path(Path("vendor/LICENSES/Apache-2.0.txt")))


class PySideLicenseInventoryTests(unittest.TestCase):
    def test_commercial_reference_notice_is_not_mistaken_for_selected_license(self) -> None:
        expression = "LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only"
        for component in ("PySide6_Essentials", "shiboken6"):
            with self.subTest(component=component):
                notes = "\\n".join(component_license_notes(component, expression))
                self.assertIn("LGPL-3.0-only", notes)
                self.assertIn("not the LGPL text", notes)
                self.assertIn("not evidence", notes)
                self.assertIn("LGPL-3.0-only.txt", notes)

    def test_unexpected_qt_license_expression_fails_closed(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "Unreviewed PySide6_Essentials license expression"):
            component_license_notes("PySide6_Essentials", "LicenseRef-Qt-Commercial")


if __name__ == "__main__":
    unittest.main(verbosity=2)
