"""Tests for Windows one-folder legal-file staging and ambient DLL cleanup."""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.normalize_windows_licenses import normalize


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


if __name__ == "__main__":
    unittest.main(verbosity=2)
