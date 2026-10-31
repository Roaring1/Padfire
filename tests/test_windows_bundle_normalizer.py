"""Tests for Windows one-folder notice staging and runtime DLL cleanup."""
from __future__ import annotations
import tempfile
import unittest
from pathlib import Path
from tools.normalize_windows_licenses import normalize
from tools.collect_third_party_licenses import component_license_notes, is_license_path
from tools.normalize_numpy_msvc_runtime import verify_windows_wheel_report


class NumpyWheelProvenanceTests(unittest.TestCase):
    def _report(self):
        entries = [
            ("numpy", "2.5.4", "numpy-2.5.4-cp312-cp312-win_amd64.whl", "a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a"),
            ("pyside6_essentials", "6.12.0", "pyside6_essentials-6.12.0-cp310-abi3-win_amd64.whl", "c9a95102aa23c1f86516a30d5a9be37552d324eee7e567eb7508973f1218cb42"),
            ("shiboken6", "6.12.0", "shiboken6-6.12.0-cp310-abi3-win_amd64.whl", "a1906cb8116869178c64b6bab52623bbc8b4ceed7b30bfc3c9aa0f6e67f6438e"),
        ]
        return {"install": [{
            "metadata": {"name": name, "version": version},
            "download_info": {
                "url": f"https://files.pythonhosted.org/packages/example/{filename}",
                "archive_info": {"hashes": {"sha256": digest}},
            },
        } for name, version, filename, digest in entries]}

    def test_validates_exact_numpy_pyside_and_shiboken_wheels(self) -> None:
        import json
        with tempfile.TemporaryDirectory() as temp:
            report_path = Path(temp) / "pip-report.json"
            report_path.write_text(json.dumps(self._report()), encoding="utf-8")
            result = verify_windows_wheel_report(report_path)
            self.assertEqual(result["numpy"]["sha256"], "a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a")
            self.assertEqual(result["pyside6-essentials"]["sha256"], "c9a95102aa23c1f86516a30d5a9be37552d324eee7e567eb7508973f1218cb42")
            self.assertEqual(result["shiboken6"]["sha256"], "a1906cb8116869178c64b6bab52623bbc8b4ceed7b30bfc3c9aa0f6e67f6438e")
            self.assertEqual(result["numpy"]["provenance"], "PyPI Publish Attestation")
            self.assertEqual(result["pyside6-essentials"]["provenance"], "No PyPI provenance attestation")

    def test_rejects_wrong_hash_filename_host_or_missing_wheel(self) -> None:
        import json
        report = self._report()
        cases = []
        wrong_hash = json.loads(json.dumps(report))
        wrong_hash["install"][1]["download_info"]["archive_info"]["hashes"]["sha256"] = "0" * 64
        cases.append(wrong_hash)
        wrong_name = json.loads(json.dumps(report))
        wrong_name["install"][2]["download_info"]["url"] = "https://files.pythonhosted.org/packages/example/wrong.whl"
        cases.append(wrong_name)
        wrong_host = json.loads(json.dumps(report))
        wrong_host["install"][0]["download_info"]["url"] = "https://example.invalid/numpy.whl"
        cases.append(wrong_host)
        missing = json.loads(json.dumps(report))
        missing["install"].pop(2)
        cases.append(missing)
        with tempfile.TemporaryDirectory() as temp:
            for index, value in enumerate(cases):
                with self.subTest(case=index):
                    path = Path(temp) / f"report-{index}.json"
                    path.write_text(json.dumps(value), encoding="utf-8")
                    with self.assertRaises(RuntimeError):
                        verify_windows_wheel_report(path)


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
        (legal / "WINDOWS_RUNTIME_PATCHES.md").write_text("Patched imports", encoding="utf-8")
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

    def test_standard_app_local_crt_removed(self) -> None:
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
            normalize(bundle, root)
            forbidden = {
                "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll",
                "vcruntime140.dll", "vcruntime140_1.dll", "ucrtbase.dll",
            }
            self.assertFalse([p for p in bundle.rglob("*.dll") if p.name.casefold() in forbidden])

    def test_numpy_hash_renamed_runtime_dll_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self._source(root)
            bundle, internal = self._bundle(root)
            numpy_dll = internal / "numpy.libs" / "msvcp140-a4c2229bdc2a2a630acdc095b4d86008.dll"
            numpy_dll.parent.mkdir(parents=True)
            numpy_dll.write_bytes(b"NumPy wheel import")
            with self.assertRaisesRegex(RuntimeError, "hash-renamed Microsoft CRT DLLs remain"):
                normalize(bundle, root)

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
