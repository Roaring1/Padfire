"""Tests for Windows one-folder legal-file staging and ambient DLL cleanup."""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.normalize_windows_licenses import normalize
from tools.collect_third_party_licenses import component_license_notes, is_license_path


class NormalizeWindowsLicensesTests(unittest.TestCase):
    def test_removes_ambient_openssl_but_preserves_cpython_openssl(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            bundle = root / "Padfire"
            internal = bundle / "_internal"
            legal = internal / "ThirdPartyLicenses"
            legal.mkdir(parents=True)
            (root / "LICENSE").write_text("MIT", encoding="utf-8")
            (root / "THIRD_PARTY_NOTICES.md").write_text("Notices", encoding="utf-8")
            (legal / "COMPONENTS.md").write_text("Components", encoding="utf-8")

            for name in (
                "libcrypto-3-x64.dll",
                "libssl-3-x64.dll",
                "libcrypto-3.dll",
                "libssl-3.dll",
            ):
                (internal / name).write_bytes(b"test binary")

            normalize(bundle, root)

            self.assertFalse((internal / "libcrypto-3-x64.dll").exists())
            self.assertFalse((internal / "libssl-3-x64.dll").exists())
            self.assertTrue((internal / "libcrypto-3.dll").is_file())
            self.assertTrue((internal / "libssl-3.dll").is_file())
            self.assertTrue((bundle / "ThirdPartyLicenses" / "COMPONENTS.md").is_file())
            self.assertTrue((bundle / "LICENSE").is_file())
            self.assertTrue((bundle / "THIRD_PARTY_NOTICES.md").is_file())


class LicensePathDiscoveryTests(unittest.TestCase):
    def test_license_directory_files_are_discovered(self) -> None:
        self.assertTrue(is_license_path(Path("LICENSES/LGPL-3.0-only.txt")))
        self.assertTrue(is_license_path(Path("pkg.dist-info/licenses/LicenseRef-Qt-Commercial.txt")))
        self.assertTrue(is_license_path(Path("LICENSE")))
        self.assertFalse(is_license_path(Path("PySide6/Qt6Core.dll")))

class PySideLicenseInventoryTests(unittest.TestCase):
    def test_commercial_reference_notice_is_not_mistaken_for_selected_license(self) -> None:
        expression = "LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only"
        for component in ("PySide6_Essentials", "shiboken6"):
            with self.subTest(component=component):
                notes = "\\n".join(component_license_notes(component, expression))
                self.assertIn("selects `LGPL-3.0-only`", notes)
                self.assertIn("not the LGPL text", notes)
                self.assertIn("not evidence", notes)
                self.assertIn("LGPL-3.0-only.txt", notes)

    def test_unexpected_qt_license_expression_fails_closed(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "Unreviewed PySide6_Essentials license expression"):
            component_license_notes("PySide6_Essentials", "LicenseRef-Qt-Commercial")



if __name__ == "__main__":
    unittest.main(verbosity=2)
